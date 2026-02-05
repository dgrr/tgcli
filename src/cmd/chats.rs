use crate::app::App;
use crate::out;
use crate::store::Store;
use crate::Cli;
use anyhow::Result;
use clap::{ArgAction, Subcommand};
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
        /// Filter by folder ID
        #[arg(long)]
        folder: Option<i32>,
        /// Show only archived chats (shortcut for --folder 1)
        #[arg(long)]
        archived: bool,
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
    /// Archive chats (move to Archive folder)
    Archive {
        /// Chat ID(s) to archive (can be specified multiple times)
        #[arg(long, action = ArgAction::Append)]
        id: Vec<i64>,
    },
    /// Unarchive chats (move out of Archive folder)
    Unarchive {
        /// Chat ID(s) to unarchive (can be specified multiple times)
        #[arg(long, action = ArgAction::Append)]
        id: Vec<i64>,
    },
    /// Pin chats
    Pin {
        /// Chat ID(s) to pin (can be specified multiple times)
        #[arg(long, action = ArgAction::Append)]
        id: Vec<i64>,
        /// Folder ID (0 = main chat list, 1 = archive, etc.)
        #[arg(long, default_value = "0")]
        folder: i32,
    },
    /// Unpin chats
    Unpin {
        /// Chat ID(s) to unpin (can be specified multiple times)
        #[arg(long, action = ArgAction::Append)]
        id: Vec<i64>,
        /// Folder ID (0 = main chat list, 1 = archive, etc.)
        #[arg(long, default_value = "0")]
        folder: i32,
    },
    /// Ban a user from a group/channel
    Ban {
        /// Chat ID (group or channel)
        #[arg(long)]
        chat: i64,
        /// User ID to ban
        #[arg(long)]
        user: i64,
        /// Duration of ban (e.g., "1d", "1h", "forever") - default: forever
        #[arg(long, default_value = "forever")]
        duration: String,
    },
    /// Kick a user from a group/channel (they can rejoin)
    Kick {
        /// Chat ID (group or channel)
        #[arg(long)]
        chat: i64,
        /// User ID to kick
        #[arg(long)]
        user: i64,
    },
    /// Unban a user from a group/channel
    Unban {
        /// Chat ID (group or channel)
        #[arg(long)]
        chat: i64,
        /// User ID to unban
        #[arg(long)]
        user: i64,
    },
    /// Promote a user to admin in a group/channel
    Promote {
        /// Chat ID (group or channel)
        #[arg(long)]
        chat: i64,
        /// User ID to promote
        #[arg(long)]
        user: i64,
        /// Admin title (e.g., "Moderator")
        #[arg(long)]
        title: Option<String>,
    },
    /// Demote an admin to regular user
    Demote {
        /// Chat ID (group or channel)
        #[arg(long)]
        chat: i64,
        /// User ID to demote
        #[arg(long)]
        user: i64,
    },
    /// Search for chats by name via Telegram API
    Search {
        /// Search query
        query: String,
        /// Limit results
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Create a new group or channel
    Create {
        /// Name of the group or channel
        #[arg(long)]
        name: String,
        /// Type: "group" or "channel"
        #[arg(long, value_parser = ["group", "channel"])]
        r#type: String,
        /// Description/about text
        #[arg(long)]
        description: Option<String>,
    },
    /// Join a group or channel
    Join {
        /// Invite link (e.g., https://t.me/+ABC123 or https://t.me/joinchat/ABC123)
        #[arg(long, conflicts_with = "username")]
        link: Option<String>,
        /// Public username (e.g., "durov" or "@durov")
        #[arg(long, conflicts_with = "link")]
        username: Option<String>,
    },
    /// Leave a group or channel
    Leave {
        /// Chat ID to leave
        #[arg(long)]
        id: i64,
    },
    /// Get or create invite links for a chat
    InviteLink {
        /// Chat ID
        #[arg(long)]
        id: i64,
        /// Create a new invite link (instead of getting existing)
        #[arg(long)]
        create: bool,
        /// Expiration duration for new link (e.g., "1h", "1d", "7d", "30d")
        #[arg(long)]
        expire: Option<String>,
        /// Maximum number of uses for new link (0 = unlimited)
        #[arg(long)]
        limit: Option<i32>,
    },
    /// Mute notifications for a chat
    Mute {
        /// Chat ID to mute
        #[arg(long)]
        id: i64,
        /// Mute duration (e.g., "1h", "8h", "1d", "forever")
        #[arg(long, default_value = "forever")]
        duration: String,
    },
    /// Unmute notifications for a chat
    Unmute {
        /// Chat ID to unmute
        #[arg(long)]
        id: i64,
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
        ChatsCommand::List {
            query,
            limit,
            folder,
            archived,
        } => {
            // If filtering by folder or archived, we need to fetch from Telegram API
            let folder_id = if *archived { Some(1) } else { *folder };

            if let Some(fid) = folder_id {
                // Fetch chats from folder via Telegram API
                list_folder_chats(cli, &store, fid, query.as_deref(), *limit).await?;
            } else {
                // Use local store
                let chats = store.list_chats(query.as_deref(), *limit).await?;

                if cli.output.is_json() {
                    out::write_json(&chats)?;
                } else {
                    println!("{:<12} {:<30} {:<16} LAST MESSAGE", "KIND", "NAME", "ID");
                    for c in &chats {
                        let name = out::truncate(&c.name, 28);
                        let ts = c
                            .last_message_ts
                            .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
                            .unwrap_or_default();
                        let kind_display = if c.is_forum {
                            format!("{}[forum]", c.kind)
                        } else {
                            c.kind.clone()
                        };
                        println!("{:<12} {:<30} {:<16} {}", kind_display, name, c.id, ts);
                    }
                }
            }
        }
        ChatsCommand::Show { id } => {
            let chat = store.get_chat(*id).await?;
            match chat {
                Some(c) => {
                    if cli.output.is_json() {
                        out::write_json(&c)?;
                    } else {
                        println!("ID: {}", c.id);
                        println!("Kind: {}", c.kind);
                        println!("Name: {}", c.name);
                        if let Some(u) = &c.username {
                            println!("Username: @{}", u);
                        }
                        if c.is_forum {
                            println!("Forum: yes");
                        }
                        if let Some(ts) = c.last_message_ts {
                            println!("Last message: {}", ts.to_rfc3339());
                        }
                    }
                }
                None => {
                    anyhow::bail!(
                        "Chat {} not found. Run `tgcli sync` to refresh your chat list.",
                        id
                    );
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

            if cli.output.is_json() {
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
                        anyhow::bail!("Could not resolve username @{}. The username may not exist or may be misspelled.", username);
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

            if cli.output.is_json() {
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
        ChatsCommand::Archive { id } => {
            if id.is_empty() {
                anyhow::bail!("At least one --id is required");
            }
            batch_archive(cli, id, true).await?;
        }
        ChatsCommand::Unarchive { id } => {
            if id.is_empty() {
                anyhow::bail!("At least one --id is required");
            }
            batch_archive(cli, id, false).await?;
        }
        ChatsCommand::Pin { id, folder } => {
            if id.is_empty() {
                anyhow::bail!("At least one --id is required");
            }
            batch_pin(cli, id, true, *folder).await?;
        }
        ChatsCommand::Unpin { id, folder } => {
            if id.is_empty() {
                anyhow::bail!("At least one --id is required");
            }
            batch_pin(cli, id, false, *folder).await?;
        }
        ChatsCommand::Ban {
            chat,
            user,
            duration,
        } => {
            let app = App::new(cli).await?;
            let until_date = parse_ban_duration(duration)?;
            app.ban_user(*chat, *user, until_date).await?;

            if cli.output.is_json() {
                out::write_json(&serde_json::json!({
                    "action": "ban",
                    "chat_id": chat,
                    "user_id": user,
                    "until_date": until_date,
                }))?;
            } else {
                let duration_str = if until_date == 0 {
                    "forever".to_string()
                } else {
                    let dt = chrono::DateTime::from_timestamp(until_date as i64, 0)
                        .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    format!("until {}", dt)
                };
                println!("Banned user {} from chat {} ({})", user, chat, duration_str);
            }
        }
        ChatsCommand::Kick { chat, user } => {
            let app = App::new(cli).await?;
            app.kick_user(*chat, *user).await?;

            if cli.output.is_json() {
                out::write_json(&serde_json::json!({
                    "action": "kick",
                    "chat_id": chat,
                    "user_id": user,
                }))?;
            } else {
                println!("Kicked user {} from chat {}", user, chat);
            }
        }
        ChatsCommand::Unban { chat, user } => {
            let app = App::new(cli).await?;
            app.unban_user(*chat, *user).await?;

            if cli.output.is_json() {
                out::write_json(&serde_json::json!({
                    "action": "unban",
                    "chat_id": chat,
                    "user_id": user,
                }))?;
            } else {
                println!("Unbanned user {} from chat {}", user, chat);
            }
        }
        ChatsCommand::Promote { chat, user, title } => {
            let app = App::new(cli).await?;
            app.promote_user(*chat, *user, title.as_deref()).await?;

            if cli.output.is_json() {
                out::write_json(&serde_json::json!({
                    "action": "promote",
                    "chat_id": chat,
                    "user_id": user,
                    "title": title,
                }))?;
            } else if let Some(t) = title {
                println!(
                    "Promoted user {} to admin in chat {} (title: {})",
                    user, chat, t
                );
            } else {
                println!("Promoted user {} to admin in chat {}", user, chat);
            }
        }
        ChatsCommand::Demote { chat, user } => {
            let app = App::new(cli).await?;
            app.demote_user(*chat, *user).await?;

            if cli.output.is_json() {
                out::write_json(&serde_json::json!({
                    "action": "demote",
                    "chat_id": chat,
                    "user_id": user,
                }))?;
            } else {
                println!("Demoted user {} in chat {}", user, chat);
            }
        }
        ChatsCommand::Search { query, limit } => {
            let app = App::new(cli).await?;
            let results = app.search_chats(query, *limit).await?;

            if cli.output.is_json() {
                out::write_json(&serde_json::json!({
                    "query": query,
                    "count": results.len(),
                    "chats": results,
                }))?;
            } else {
                println!("Search results for \"{}\":\n", query);
                println!("{:<12} {:<30} {:<16} USERNAME", "KIND", "NAME", "ID");
                for c in &results {
                    println!(
                        "{:<12} {:<30} {:<16} {}",
                        c.kind,
                        out::truncate(&c.name, 28),
                        c.id,
                        c.username.as_deref().unwrap_or("-")
                    );
                }
            }
        }
        ChatsCommand::Create {
            name,
            r#type,
            description,
        } => {
            let app = App::new(cli).await?;
            let result = app
                .create_chat(name, r#type, description.as_deref())
                .await?;

            if cli.output.is_json() {
                out::write_json(&serde_json::json!({
                    "created": true,
                    "id": result.id,
                    "kind": result.kind,
                    "name": result.name,
                }))?;
            } else {
                println!(
                    "Created {} \"{}\" (ID: {})",
                    result.kind, result.name, result.id
                );
            }
        }
        ChatsCommand::Join { link, username } => {
            let app = App::new(cli).await?;
            let result = app.join_chat(link.as_deref(), username.as_deref()).await?;

            if cli.output.is_json() {
                out::write_json(&serde_json::json!({
                    "joined": true,
                    "id": result.id,
                    "kind": result.kind,
                    "name": result.name,
                }))?;
            } else {
                println!(
                    "Joined {} \"{}\" (ID: {})",
                    result.kind, result.name, result.id
                );
            }
        }
        ChatsCommand::Leave { id } => {
            let app = App::new(cli).await?;
            app.leave_chat(*id).await?;

            if cli.output.is_json() {
                out::write_json(&serde_json::json!({
                    "left": true,
                    "chat_id": id,
                }))?;
            } else {
                println!("Left chat {}", id);
            }
        }
        ChatsCommand::InviteLink {
            id,
            create,
            expire,
            limit,
        } => {
            let app = App::new(cli).await?;

            if *create {
                // Parse expire duration
                let expire_date = if let Some(exp) = expire {
                    Some(parse_expire_duration(exp)?)
                } else {
                    None
                };

                let result = app.create_invite_link(*id, expire_date, *limit).await?;

                if cli.output.is_json() {
                    out::write_json(&serde_json::json!({
                        "created": true,
                        "link": result.link,
                        "expire_date": result.expire_date,
                        "usage_limit": result.usage_limit,
                    }))?;
                } else {
                    println!("Invite link: {}", result.link);
                    if let Some(exp) = result.expire_date {
                        println!("Expires: {}", exp);
                    }
                    if let Some(lim) = result.usage_limit {
                        println!("Usage limit: {}", lim);
                    }
                }
            } else {
                let link = app.get_invite_link(*id).await?;

                if cli.output.is_json() {
                    out::write_json(&serde_json::json!({
                        "link": link,
                    }))?;
                } else {
                    println!("Invite link: {}", link);
                }
            }
        }
        ChatsCommand::Mute { id, duration } => {
            let app = App::new(cli).await?;
            let mute_until = parse_mute_duration(duration)?;
            app.mute_chat(*id, mute_until).await?;

            if cli.output.is_json() {
                out::write_json(&serde_json::json!({
                    "muted": true,
                    "chat_id": id,
                    "until": if mute_until == i32::MAX { "forever".to_string() } else { mute_until.to_string() },
                }))?;
            } else if mute_until == i32::MAX {
                println!("Muted chat {} forever", id);
            } else {
                println!("Muted chat {} until {}", id, mute_until);
            }
        }
        ChatsCommand::Unmute { id } => {
            let app = App::new(cli).await?;
            app.unmute_chat(*id).await?;

            if cli.output.is_json() {
                out::write_json(&serde_json::json!({
                    "unmuted": true,
                    "chat_id": id,
                }))?;
            } else {
                println!("Unmuted chat {}", id);
            }
        }
    }
    Ok(())
}

/// Parse ban duration string to Unix timestamp (0 = forever)
fn parse_ban_duration(duration: &str) -> Result<i32> {
    if duration == "forever" || duration == "0" {
        return Ok(0);
    }

    let now = chrono::Utc::now();
    let secs = if duration.ends_with('d') {
        duration
            .trim_end_matches('d')
            .parse::<i64>()
            .map(|d| d * 86400)?
    } else if duration.ends_with('h') {
        duration
            .trim_end_matches('h')
            .parse::<i64>()
            .map(|h| h * 3600)?
    } else if duration.ends_with('m') {
        duration
            .trim_end_matches('m')
            .parse::<i64>()
            .map(|m| m * 60)?
    } else {
        // Try parsing as seconds
        duration.parse::<i64>()?
    };

    Ok((now.timestamp() + secs) as i32)
}

/// Parse mute duration string to Unix timestamp (i32::MAX = forever)
fn parse_mute_duration(duration: &str) -> Result<i32> {
    if duration == "forever" {
        return Ok(i32::MAX);
    }

    let now = chrono::Utc::now();
    let secs = if duration.ends_with('d') {
        duration
            .trim_end_matches('d')
            .parse::<i64>()
            .map(|d| d * 86400)?
    } else if duration.ends_with('h') {
        duration
            .trim_end_matches('h')
            .parse::<i64>()
            .map(|h| h * 3600)?
    } else {
        anyhow::bail!("Invalid duration format. Use '1h', '8h', '1d', or 'forever'");
    };

    Ok((now.timestamp() + secs) as i32)
}

/// Parse expire duration string to Unix timestamp
fn parse_expire_duration(duration: &str) -> Result<i32> {
    let now = chrono::Utc::now();
    let secs = if duration.ends_with('d') {
        duration
            .trim_end_matches('d')
            .parse::<i64>()
            .map(|d| d * 86400)?
    } else if duration.ends_with('h') {
        duration
            .trim_end_matches('h')
            .parse::<i64>()
            .map(|h| h * 3600)?
    } else {
        anyhow::bail!("Invalid duration format. Use '1h', '1d', '7d', '30d'");
    };

    Ok((now.timestamp() + secs) as i32)
}

/// List chats from a specific folder
async fn list_folder_chats(
    cli: &Cli,
    _store: &Store,
    folder_id: i32,
    query: Option<&str>,
    limit: i64,
) -> Result<()> {
    let app = App::new(cli).await?;

    // Fetch dialogs with folder filter
    let request = tl::functions::messages::GetDialogs {
        exclude_pinned: false,
        folder_id: Some(folder_id),
        offset_date: 0,
        offset_id: 0,
        offset_peer: tl::enums::InputPeer::Empty,
        limit: limit as i32,
        hash: 0,
    };

    let result = app.tg.client.invoke(&request).await?;

    #[derive(Serialize)]
    struct FolderChat {
        id: i64,
        kind: String,
        name: String,
        username: Option<String>,
    }

    let mut chats: Vec<FolderChat> = Vec::new();

    // Extract chats from the response
    let (dialogs, users, chat_list) = match result {
        tl::enums::messages::Dialogs::Dialogs(d) => (d.dialogs, d.users, d.chats),
        tl::enums::messages::Dialogs::Slice(d) => (d.dialogs, d.users, d.chats),
        tl::enums::messages::Dialogs::NotModified(_) => {
            if cli.output.is_json() {
                out::write_json(&Vec::<FolderChat>::new())?;
            } else {
                println!("No chats in folder {}", folder_id);
            }
            return Ok(());
        }
    };

    // Build lookup maps for users and chats
    let mut user_map: std::collections::HashMap<i64, (String, Option<String>)> =
        std::collections::HashMap::new();
    let mut chat_map: std::collections::HashMap<i64, (String, Option<String>, String)> =
        std::collections::HashMap::new();

    for user in &users {
        if let tl::enums::User::User(u) = user {
            let name = format!(
                "{} {}",
                u.first_name.as_deref().unwrap_or(""),
                u.last_name.as_deref().unwrap_or("")
            )
            .trim()
            .to_string();
            user_map.insert(u.id, (name, u.username.clone()));
        }
    }

    for chat in &chat_list {
        match chat {
            tl::enums::Chat::Chat(c) => {
                chat_map.insert(c.id, (c.title.clone(), None, "group".to_string()));
            }
            tl::enums::Chat::Channel(c) => {
                let kind = if c.broadcast { "channel" } else { "supergroup" };
                chat_map.insert(
                    c.id,
                    (c.title.clone(), c.username.clone(), kind.to_string()),
                );
            }
            _ => {}
        }
    }

    // Process dialogs
    for dialog in &dialogs {
        let peer = match dialog {
            tl::enums::Dialog::Dialog(d) => &d.peer,
            tl::enums::Dialog::Folder(_) => continue,
        };

        let (id, name, username, kind) = match peer {
            tl::enums::Peer::User(u) => {
                let (name, username) = user_map
                    .get(&u.user_id)
                    .cloned()
                    .unwrap_or_else(|| (format!("User {}", u.user_id), None));
                (u.user_id, name, username, "user".to_string())
            }
            tl::enums::Peer::Chat(c) => {
                let (name, username, kind) = chat_map
                    .get(&c.chat_id)
                    .cloned()
                    .unwrap_or_else(|| (format!("Chat {}", c.chat_id), None, "group".to_string()));
                (c.chat_id, name, username, kind)
            }
            tl::enums::Peer::Channel(c) => {
                let (name, username, kind) =
                    chat_map.get(&c.channel_id).cloned().unwrap_or_else(|| {
                        (
                            format!("Channel {}", c.channel_id),
                            None,
                            "channel".to_string(),
                        )
                    });
                (c.channel_id, name, username, kind)
            }
        };

        // Apply query filter if provided
        if let Some(q) = query {
            let q_lower = q.to_lowercase();
            let name_matches = name.to_lowercase().contains(&q_lower);
            let username_matches = username
                .as_ref()
                .map(|u| u.to_lowercase().contains(&q_lower))
                .unwrap_or(false);
            if !name_matches && !username_matches {
                continue;
            }
        }

        chats.push(FolderChat {
            id,
            kind,
            name,
            username,
        });
    }

    if cli.output.is_json() {
        out::write_json(&chats)?;
    } else {
        let folder_name = if folder_id == 1 {
            "Archive"
        } else {
            &format!("Folder {}", folder_id)
        };
        println!("Chats in {} ({} total):\n", folder_name, chats.len());
        println!("{:<12} {:<30} {:<16} USERNAME", "KIND", "NAME", "ID");
        for c in &chats {
            println!(
                "{:<12} {:<30} {:<16} {}",
                c.kind,
                out::truncate(&c.name, 28),
                c.id,
                c.username.as_deref().unwrap_or("-")
            );
        }
    }

    Ok(())
}

/// Batch archive or unarchive multiple chats in a single API call
async fn batch_archive(cli: &Cli, chat_ids: &[i64], archive: bool) -> Result<()> {
    let app = App::new(cli).await?;

    // folder_id = 1 for Archive, 0 for main chat list
    let folder_id = if archive { 1 } else { 0 };

    // Resolve all chats to InputPeers and build folder_peers
    let mut folder_peers = Vec::with_capacity(chat_ids.len());
    let mut resolved_ids = Vec::with_capacity(chat_ids.len());

    for &chat_id in chat_ids {
        match resolve_chat_to_input_peer(&app, chat_id).await {
            Ok(input_peer) => {
                let folder_peer = tl::types::InputFolderPeer {
                    peer: input_peer,
                    folder_id,
                };
                folder_peers.push(tl::enums::InputFolderPeer::Peer(folder_peer));
                resolved_ids.push(chat_id);
            }
            Err(e) => {
                eprintln!("Warning: Could not resolve chat {}: {}", chat_id, e);
            }
        }
    }

    if folder_peers.is_empty() {
        anyhow::bail!("No chats could be resolved");
    }

    // Single API call for all chats
    let request = tl::functions::folders::EditPeerFolders { folder_peers };
    app.tg.client.invoke(&request).await?;

    let action = if archive { "Archived" } else { "Unarchived" };

    if cli.output.is_json() {
        let results: Vec<_> = resolved_ids
            .iter()
            .map(|&id| {
                serde_json::json!({
                    "chat_id": id,
                    "success": true,
                })
            })
            .collect();
        out::write_json(&serde_json::json!({
            "action": action.to_lowercase(),
            "count": resolved_ids.len(),
            "results": results,
        }))?;
    } else {
        for &chat_id in &resolved_ids {
            let chat_name = app
                .store
                .get_chat(chat_id)
                .await?
                .map(|c| c.name)
                .unwrap_or_else(|| format!("Chat {}", chat_id));
            println!("{} \"{}\" ({})", action, chat_name, chat_id);
        }
    }

    Ok(())
}

/// Batch pin or unpin multiple chats
/// Note: Telegram API doesn't have a batch pin endpoint, so we process sequentially
async fn batch_pin(cli: &Cli, chat_ids: &[i64], pin: bool, folder_id: i32) -> Result<()> {
    let app = App::new(cli).await?;

    let action = if pin { "Pinned" } else { "Unpinned" };
    let mut results = Vec::with_capacity(chat_ids.len());

    for &chat_id in chat_ids {
        // Resolve chat to InputPeer
        let input_peer = match resolve_chat_to_input_peer(&app, chat_id).await {
            Ok(peer) => peer,
            Err(e) => {
                eprintln!("Warning: Could not resolve chat {}: {}", chat_id, e);
                results.push((chat_id, false, Some(e.to_string())));
                continue;
            }
        };

        // Create InputDialogPeer - folder_id only affects which pin list, not the peer itself
        let input_dialog_peer =
            tl::enums::InputDialogPeer::Peer(tl::types::InputDialogPeer { peer: input_peer });

        let request = tl::functions::messages::ToggleDialogPin {
            pinned: pin,
            peer: input_dialog_peer,
        };

        match app.tg.client.invoke(&request).await {
            Ok(_) => {
                results.push((chat_id, true, None));
            }
            Err(e) => {
                eprintln!(
                    "Warning: Failed to {} chat {}: {}",
                    action.to_lowercase(),
                    chat_id,
                    e
                );
                results.push((chat_id, false, Some(e.to_string())));
            }
        }
    }

    let success_count = results.iter().filter(|(_, success, _)| *success).count();

    if cli.output.is_json() {
        let json_results: Vec<_> = results
            .iter()
            .map(|(id, success, error)| {
                let mut obj = serde_json::json!({
                    "chat_id": id,
                    "success": success,
                });
                if let Some(err) = error {
                    obj["error"] = serde_json::json!(err);
                }
                obj
            })
            .collect();
        out::write_json(&serde_json::json!({
            "action": action.to_lowercase(),
            "folder_id": folder_id,
            "count": success_count,
            "results": json_results,
        }))?;
    } else {
        for (chat_id, success, _) in &results {
            if *success {
                let chat_name = app
                    .store
                    .get_chat(*chat_id)
                    .await?
                    .map(|c| c.name)
                    .unwrap_or_else(|| format!("Chat {}", chat_id));
                if folder_id != 0 {
                    println!(
                        "{} \"{}\" ({}) in folder {}",
                        action, chat_name, chat_id, folder_id
                    );
                } else {
                    println!("{} \"{}\" ({})", action, chat_name, chat_id);
                }
            }
        }
    }

    if success_count == 0 {
        anyhow::bail!("No chats were successfully {}d", action.to_lowercase());
    }

    Ok(())
}

/// Resolve a chat ID to an InputPeer
async fn resolve_chat_to_input_peer(app: &App, chat_id: i64) -> Result<tl::enums::InputPeer> {
    // First check session for channel
    let channel_peer_id = PeerId::channel(chat_id);
    if let Some(info) = app.tg.session.peer(channel_peer_id) {
        let peer_ref = PeerRef {
            id: channel_peer_id,
            auth: info.auth(),
        };
        return Ok(peer_ref.into());
    }

    // Try as user
    let user_peer_id = PeerId::user(chat_id);
    if let Some(info) = app.tg.session.peer(user_peer_id) {
        let peer_ref = PeerRef {
            id: user_peer_id,
            auth: info.auth(),
        };
        return Ok(peer_ref.into());
    }

    // Try as small group chat
    if chat_id > 0 && chat_id <= 999999999999 {
        let chat_peer_id = PeerId::chat(chat_id);
        if let Some(info) = app.tg.session.peer(chat_peer_id) {
            let peer_ref = PeerRef {
                id: chat_peer_id,
                auth: info.auth(),
            };
            return Ok(peer_ref.into());
        }
    }

    // Try to resolve via dialogs
    let mut dialogs = app.tg.client.iter_dialogs();
    while let Some(dialog) = dialogs.next().await? {
        let peer = dialog.peer();
        if peer.id().bare_id() == chat_id {
            return Ok(PeerRef::from(peer).into());
        }
    }

    anyhow::bail!(
        "Chat {} not found in your chat list. Run `tgcli sync` to refresh, or check that the chat ID is correct.",
        chat_id
    );
}
