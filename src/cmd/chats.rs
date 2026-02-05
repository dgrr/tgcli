use crate::app::App;
use crate::out;
use crate::store::Store;
use crate::Cli;
use anyhow::Result;
use clap::Subcommand;
use grammers_session::defs::{PeerAuth, PeerId, PeerRef};
use grammers_session::Session;
use grammers_tl_types as tl;
use serde::Serialize;

#[derive(Subcommand, Debug, Clone)]
pub enum ChatsCommand {
    /// List chats
    List {
        /// Search query
        #[arg(long)]
        query: Option<String>,
        /// Limit results
        #[arg(long, default_value = "50")]
        limit: i64,
    },
    /// Show a single chat
    Show {
        /// Chat ID
        #[arg(long)]
        id: i64,
    },
    /// Delete a chat from local database
    Delete {
        /// Chat ID to delete
        chat_id: i64,
        /// Soft delete (local DB only, default behavior)
        #[arg(long, default_value = "true")]
        soft: bool,
        /// Hard delete (also delete from Telegram) - NOT IMPLEMENTED
        #[arg(long, conflicts_with = "soft")]
        hard: bool,
    },
    /// List members of a group or channel
    Members {
        /// Chat ID (group or channel)
        #[arg(long)]
        id: i64,
        /// Limit results (0 = all)
        #[arg(long, default_value = "100")]
        limit: usize,
    },
}

#[derive(Serialize)]
struct MemberInfo {
    id: i64,
    username: Option<String>,
    first_name: Option<String>,
    last_name: Option<String>,
    status: String,
    role: String,
}

fn format_user_status(status: &tl::enums::UserStatus) -> String {
    match status {
        tl::enums::UserStatus::Empty => "unknown".to_string(),
        tl::enums::UserStatus::Online(_) => "online".to_string(),
        tl::enums::UserStatus::Offline(o) => {
            let ts = chrono::DateTime::from_timestamp(o.was_online as i64, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "offline".to_string());
            format!("offline ({})", ts)
        }
        tl::enums::UserStatus::Recently(_) => "recently".to_string(),
        tl::enums::UserStatus::LastWeek(_) => "last week".to_string(),
        tl::enums::UserStatus::LastMonth(_) => "last month".to_string(),
    }
}

fn format_role(role: &grammers_client::types::Role) -> String {
    use grammers_client::types::Role;
    match role {
        Role::User(_) => "member".to_string(),
        Role::Creator(_) => "creator".to_string(),
        Role::Admin(_) => "admin".to_string(),
        Role::Banned(_) => "banned".to_string(),
        Role::Left(_) => "left".to_string(),
        _ => "unknown".to_string(), // Role is non-exhaustive
    }
}

pub async fn run(cli: &Cli, cmd: &ChatsCommand) -> Result<()> {
    let store = Store::open(&cli.store_dir()).await?;

    match cmd {
        ChatsCommand::List { query, limit } => {
            let chats = store.list_chats(query.as_deref(), *limit).await?;

            if cli.json {
                out::write_json(&chats)?;
            } else {
                println!("{:<6} {:<30} {:<16} LAST MESSAGE", "KIND", "NAME", "ID");
                for c in &chats {
                    let name = out::truncate(&c.name, 28);
                    let ts = c
                        .last_message_ts
                        .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_default();
                    println!("{:<6} {:<30} {:<16} {}", c.kind, name, c.id, ts);
                }
            }
        }
        ChatsCommand::Show { id } => {
            let chat = store.get_chat(*id).await?;
            match chat {
                Some(c) => {
                    if cli.json {
                        out::write_json(&c)?;
                    } else {
                        println!("ID: {}", c.id);
                        println!("Kind: {}", c.kind);
                        println!("Name: {}", c.name);
                        if let Some(u) = &c.username {
                            println!("Username: @{}", u);
                        }
                        if let Some(ts) = c.last_message_ts {
                            println!("Last message: {}", ts.to_rfc3339());
                        }
                    }
                }
                None => {
                    anyhow::bail!("Chat {} not found", id);
                }
            }
        }
        ChatsCommand::Delete {
            chat_id,
            soft: _,
            hard,
        } => {
            if *hard {
                anyhow::bail!("Hard delete not implemented. Use --soft (default) to delete from local DB only.");
            }

            // Get chat info before deletion for confirmation message
            let chat = store.get_chat(*chat_id).await?;
            let chat_name = chat
                .as_ref()
                .map(|c| c.name.clone())
                .unwrap_or_else(|| format!("(unknown chat {})", chat_id));

            // Delete messages first, then the chat
            let messages_deleted = store.delete_messages_by_chat(*chat_id).await?;
            let chat_deleted = store.delete_chat(*chat_id).await?;

            if cli.json {
                out::write_json(&serde_json::json!({
                    "chat_id": chat_id,
                    "chat_name": chat_name,
                    "chat_deleted": chat_deleted,
                    "messages_deleted": messages_deleted,
                }))?;
            } else if chat_deleted {
                println!("Deleted chat \"{}\" ({})", chat_name, chat_id);
                println!("Deleted {} message(s)", messages_deleted);
            } else {
                println!("Chat {} not found in local database", chat_id);
                if messages_deleted > 0 {
                    println!("Deleted {} orphaned message(s)", messages_deleted);
                }
            }
        }
        ChatsCommand::Members { id, limit } => {
            // Look up the chat to get its name and username for display
            let chat = store.get_chat(*id).await?;
            let chat_name = chat
                .as_ref()
                .map(|c| c.name.clone())
                .unwrap_or_else(|| format!("Chat {}", id));
            let chat_kind = chat.as_ref().map(|c| c.kind.as_str()).unwrap_or("unknown");
            let chat_username = chat.as_ref().and_then(|c| c.username.clone());

            if chat_kind == "user" {
                anyhow::bail!("Cannot list members of a private chat (user {})", id);
            }

            // Connect to Telegram API
            let app = App::new(cli).await?;

            // Try to resolve the peer - we need the access_hash for channels/megagroups
            // First try via username if available (most reliable), then via session
            let peer_ref = if let Some(ref username) = chat_username {
                // Resolve via username
                match app.tg.client.resolve_username(username).await? {
                    Some(peer) => PeerRef::from(peer),
                    None => {
                        anyhow::bail!("Could not resolve username @{}", username);
                    }
                }
            } else {
                // Try to get from session, fall back to trying different ID types
                let channel_peer_id = PeerId::channel(*id);
                let chat_peer_id = if *id > 0 && *id <= 999999999999 {
                    Some(PeerId::chat(*id))
                } else {
                    None
                };

                // Check session for channel first
                if let Some(info) = app.tg.session.peer(channel_peer_id) {
                    PeerRef {
                        id: channel_peer_id,
                        auth: info.auth(),
                    }
                } else if let Some(chat_id) = chat_peer_id {
                    // Try as small group chat (no access_hash needed)
                    PeerRef {
                        id: chat_id,
                        auth: PeerAuth::default(),
                    }
                } else {
                    // Last resort: try channel with no access_hash
                    // This will likely fail but provides a clear error
                    PeerRef {
                        id: channel_peer_id,
                        auth: PeerAuth::default(),
                    }
                }
            };

            let mut participants = app.tg.client.iter_participants(peer_ref);

            let mut members: Vec<MemberInfo> = Vec::new();
            let mut count = 0usize;

            while let Some(participant) = participants.next().await? {
                let user = &participant.user;
                let member = MemberInfo {
                    id: user.bare_id(),
                    username: user.username().map(|s| s.to_string()),
                    first_name: user.first_name().map(|s| s.to_string()),
                    last_name: user.last_name().map(|s| s.to_string()),
                    status: format_user_status(user.status()),
                    role: format_role(&participant.role),
                };
                members.push(member);
                count += 1;

                // Check limit (0 = unlimited)
                if *limit > 0 && count >= *limit {
                    break;
                }
            }

            if cli.json {
                out::write_json(&serde_json::json!({
                    "chat_id": id,
                    "chat_name": chat_name,
                    "count": members.len(),
                    "members": members,
                }))?;
            } else {
                println!(
                    "Members of \"{}\" ({}) - {} total:\n",
                    chat_name,
                    id,
                    members.len()
                );
                println!(
                    "{:<12} {:<20} {:<20} {:<20} {:<10} STATUS",
                    "ID", "USERNAME", "FIRST_NAME", "LAST_NAME", "ROLE"
                );
                for m in &members {
                    println!(
                        "{:<12} {:<20} {:<20} {:<20} {:<10} {}",
                        m.id,
                        m.username.as_deref().unwrap_or("-"),
                        out::truncate(m.first_name.as_deref().unwrap_or("-"), 18),
                        out::truncate(m.last_name.as_deref().unwrap_or("-"), 18),
                        m.role,
                        m.status,
                    );
                }
            }
        }
    }
    Ok(())
}
