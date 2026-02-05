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
    /// Archive a chat (move to Archive folder)
    Archive {
        /// Chat ID to archive
        #[arg(long)]
        id: i64,
    },
    /// Unarchive a chat (move out of Archive folder)
    Unarchive {
        /// Chat ID to unarchive
        #[arg(long)]
        id: i64,
    },
    /// Pin a chat
    Pin {
        /// Chat ID to pin
        #[arg(long)]
        id: i64,
        /// Folder ID (0 = main chat list, 1 = archive, etc.)
        #[arg(long, default_value = "0")]
        folder: i32,
    },
    /// Unpin a chat
    Unpin {
        /// Chat ID to unpin
        #[arg(long)]
        id: i64,
        /// Folder ID (0 = main chat list, 1 = archive, etc.)
        #[arg(long, default_value = "0")]
        folder: i32,
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

                if cli.json {
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
                    if cli.json {
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
        ChatsCommand::Archive { id } => {
            archive_chat(cli, *id, true).await?;
        }
        ChatsCommand::Unarchive { id } => {
            archive_chat(cli, *id, false).await?;
        }
        ChatsCommand::Pin { id, folder } => {
            pin_chat(cli, *id, true, *folder).await?;
        }
        ChatsCommand::Unpin { id, folder } => {
            pin_chat(cli, *id, false, *folder).await?;
        }
    }
    Ok(())
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
            if cli.json {
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

    if cli.json {
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

/// Archive or unarchive a chat
async fn archive_chat(cli: &Cli, chat_id: i64, archive: bool) -> Result<()> {
    let app = App::new(cli).await?;

    // Resolve chat to InputPeer
    let input_peer = resolve_chat_to_input_peer(&app, chat_id).await?;

    // folder_id = 1 for Archive, 0 for main chat list
    let folder_id = if archive { 1 } else { 0 };

    let folder_peer = tl::types::InputFolderPeer {
        peer: input_peer,
        folder_id,
    };

    let request = tl::functions::folders::EditPeerFolders {
        folder_peers: vec![tl::enums::InputFolderPeer::Peer(folder_peer)],
    };

    app.tg.client.invoke(&request).await?;

    let action = if archive { "Archived" } else { "Unarchived" };

    if cli.json {
        out::write_json(&serde_json::json!({
            "success": true,
            "chat_id": chat_id,
            "action": action.to_lowercase(),
        }))?;
    } else {
        // Get chat name for display
        let chat_name = app
            .store
            .get_chat(chat_id)
            .await?
            .map(|c| c.name)
            .unwrap_or_else(|| format!("Chat {}", chat_id));
        println!("{} \"{}\" ({})", action, chat_name, chat_id);
    }

    Ok(())
}

/// Pin or unpin a chat
async fn pin_chat(cli: &Cli, chat_id: i64, pin: bool, folder_id: i32) -> Result<()> {
    let app = App::new(cli).await?;

    // Resolve chat to InputPeer
    let input_peer = resolve_chat_to_input_peer(&app, chat_id).await?;

    // Create InputDialogPeer with folder support
    let input_dialog_peer = if folder_id != 0 {
        tl::enums::InputDialogPeer::Folder(tl::types::InputDialogPeerFolder { folder_id })
    } else {
        tl::enums::InputDialogPeer::Peer(tl::types::InputDialogPeer { peer: input_peer })
    };

    let request = tl::functions::messages::ToggleDialogPin {
        pinned: pin,
        peer: input_dialog_peer,
    };

    app.tg.client.invoke(&request).await?;

    let action = if pin { "Pinned" } else { "Unpinned" };

    if cli.json {
        out::write_json(&serde_json::json!({
            "success": true,
            "chat_id": chat_id,
            "action": action.to_lowercase(),
            "folder_id": folder_id,
        }))?;
    } else {
        // Get chat name for display
        let chat_name = app
            .store
            .get_chat(chat_id)
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
