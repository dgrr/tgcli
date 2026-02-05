use crate::app::App;
use crate::out;
use crate::Cli;
use anyhow::Result;
use clap::Subcommand;
use grammers_session::defs::{PeerId, PeerRef};
use grammers_session::Session;
use grammers_tl_types as tl;
use serde::Serialize;

#[derive(Subcommand, Debug, Clone)]
pub enum FoldersCommand {
    /// List all folders
    List,
    /// Show chats in a folder
    Show {
        /// Folder ID
        #[arg(long)]
        id: i32,
    },
    /// Create a new folder
    Create {
        /// Folder name
        #[arg(long)]
        name: String,
        /// Optional emoticon/emoji for the folder
        #[arg(long)]
        emoticon: Option<String>,
    },
    /// Delete a folder
    Delete {
        /// Folder ID to delete
        #[arg(long)]
        id: i32,
    },
    /// Add a chat to a folder
    Add {
        /// Chat ID to add
        #[arg(long)]
        chat: i64,
        /// Folder ID
        #[arg(long)]
        folder: i32,
    },
    /// Remove a chat from a folder
    Remove {
        /// Chat ID to remove
        #[arg(long)]
        chat: i64,
        /// Folder ID
        #[arg(long)]
        folder: i32,
    },
}

#[derive(Serialize)]
struct FolderInfo {
    id: i32,
    title: String,
    emoticon: Option<String>,
    pinned_count: usize,
    include_count: usize,
    exclude_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    contacts: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    non_contacts: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    groups: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    broadcasts: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bots: Option<bool>,
}

#[derive(Serialize)]
struct FolderChat {
    id: i64,
    name: String,
    kind: String,
    pinned: bool,
}

pub async fn run(cli: &Cli, cmd: &FoldersCommand) -> Result<()> {
    match cmd {
        FoldersCommand::List => list_folders(cli).await,
        FoldersCommand::Show { id } => show_folder(cli, *id).await,
        FoldersCommand::Create { name, emoticon } => {
            create_folder(cli, name, emoticon.as_deref()).await
        }
        FoldersCommand::Delete { id } => delete_folder(cli, *id).await,
        FoldersCommand::Add { chat, folder } => add_to_folder(cli, *chat, *folder).await,
        FoldersCommand::Remove { chat, folder } => remove_from_folder(cli, *chat, *folder).await,
    }
}

async fn list_folders(cli: &Cli) -> Result<()> {
    let app = App::new(cli).await?;

    let request = tl::functions::messages::GetDialogFilters {};
    let result = app.tg.client.invoke(&request).await?;

    let filters = match result {
        tl::enums::messages::DialogFilters::Filters(f) => f.filters,
    };

    let mut folders: Vec<FolderInfo> = Vec::new();

    for filter_enum in &filters {
        match filter_enum {
            tl::enums::DialogFilter::Filter(f) => {
                let title = match &f.title {
                    tl::enums::TextWithEntities::Entities(t) => t.text.clone(),
                };
                folders.push(FolderInfo {
                    id: f.id,
                    title,
                    emoticon: f.emoticon.clone(),
                    pinned_count: f.pinned_peers.len(),
                    include_count: f.include_peers.len(),
                    exclude_count: f.exclude_peers.len(),
                    contacts: if f.contacts { Some(true) } else { None },
                    non_contacts: if f.non_contacts { Some(true) } else { None },
                    groups: if f.groups { Some(true) } else { None },
                    broadcasts: if f.broadcasts { Some(true) } else { None },
                    bots: if f.bots { Some(true) } else { None },
                });
            }
            tl::enums::DialogFilter::Default => {
                folders.push(FolderInfo {
                    id: 0,
                    title: "All Chats".to_string(),
                    emoticon: None,
                    pinned_count: 0,
                    include_count: 0,
                    exclude_count: 0,
                    contacts: None,
                    non_contacts: None,
                    groups: None,
                    broadcasts: None,
                    bots: None,
                });
            }
            tl::enums::DialogFilter::Chatlist(c) => {
                let title = match &c.title {
                    tl::enums::TextWithEntities::Entities(t) => t.text.clone(),
                };
                folders.push(FolderInfo {
                    id: c.id,
                    title,
                    emoticon: c.emoticon.clone(),
                    pinned_count: c.pinned_peers.len(),
                    include_count: c.include_peers.len(),
                    exclude_count: 0,
                    contacts: None,
                    non_contacts: None,
                    groups: None,
                    broadcasts: None,
                    bots: None,
                });
            }
        }
    }

    if cli.json {
        out::write_json(&folders)?;
    } else {
        println!(
            "{:<6} {:<30} {:<10} {:<8} {:<8} FLAGS",
            "ID", "TITLE", "EMOTICON", "PINNED", "INCLUDE"
        );
        for f in &folders {
            let mut flags = Vec::new();
            if f.contacts == Some(true) {
                flags.push("contacts");
            }
            if f.non_contacts == Some(true) {
                flags.push("non_contacts");
            }
            if f.groups == Some(true) {
                flags.push("groups");
            }
            if f.broadcasts == Some(true) {
                flags.push("broadcasts");
            }
            if f.bots == Some(true) {
                flags.push("bots");
            }
            println!(
                "{:<6} {:<30} {:<10} {:<8} {:<8} {}",
                f.id,
                out::truncate(&f.title, 28),
                f.emoticon.as_deref().unwrap_or("-"),
                f.pinned_count,
                f.include_count,
                flags.join(",")
            );
        }
    }

    Ok(())
}

async fn create_folder(cli: &Cli, name: &str, emoticon: Option<&str>) -> Result<()> {
    let app = App::new(cli).await?;

    // Get existing folders to find next available ID
    let request = tl::functions::messages::GetDialogFilters {};
    let result = app.tg.client.invoke(&request).await?;

    let filters = match result {
        tl::enums::messages::DialogFilters::Filters(f) => f.filters,
    };

    // Find max ID (folder IDs start from 2, as 0 is "All Chats" and 1 is reserved)
    let mut max_id = 1;
    for filter_enum in &filters {
        let id = match filter_enum {
            tl::enums::DialogFilter::Filter(f) => f.id,
            tl::enums::DialogFilter::Default => 0,
            tl::enums::DialogFilter::Chatlist(c) => c.id,
        };
        if id > max_id {
            max_id = id;
        }
    }
    let new_id = max_id + 1;

    // Create the title as TextWithEntities
    let title = tl::enums::TextWithEntities::Entities(tl::types::TextWithEntities {
        text: name.to_string(),
        entities: vec![],
    });

    // Create new folder filter
    let new_filter = tl::types::DialogFilter {
        contacts: false,
        non_contacts: false,
        groups: false,
        broadcasts: false,
        bots: false,
        exclude_muted: false,
        exclude_read: false,
        exclude_archived: false,
        title_noanimate: false,
        id: new_id,
        title,
        emoticon: emoticon.map(|s| s.to_string()),
        color: None,
        pinned_peers: vec![],
        include_peers: vec![],
        exclude_peers: vec![],
    };

    let create_request = tl::functions::messages::UpdateDialogFilter {
        id: new_id,
        filter: Some(tl::enums::DialogFilter::Filter(new_filter)),
    };

    app.tg.client.invoke(&create_request).await?;

    if cli.json {
        out::write_json(&serde_json::json!({
            "success": true,
            "id": new_id,
            "name": name,
            "emoticon": emoticon,
        }))?;
    } else {
        println!("Created folder '{}' with ID {}", name, new_id);
    }

    Ok(())
}

async fn delete_folder(cli: &Cli, folder_id: i32) -> Result<()> {
    let app = App::new(cli).await?;

    // Verify the folder exists first
    let request = tl::functions::messages::GetDialogFilters {};
    let result = app.tg.client.invoke(&request).await?;

    let filters = match result {
        tl::enums::messages::DialogFilters::Filters(f) => f.filters,
    };

    let mut found = false;
    let mut folder_name = String::new();
    for filter_enum in &filters {
        match filter_enum {
            tl::enums::DialogFilter::Filter(f) if f.id == folder_id => {
                found = true;
                folder_name = match &f.title {
                    tl::enums::TextWithEntities::Entities(t) => t.text.clone(),
                };
                break;
            }
            tl::enums::DialogFilter::Chatlist(c) if c.id == folder_id => {
                found = true;
                folder_name = match &c.title {
                    tl::enums::TextWithEntities::Entities(t) => t.text.clone(),
                };
                break;
            }
            tl::enums::DialogFilter::Default if folder_id == 0 => {
                anyhow::bail!("Cannot delete the default 'All Chats' folder");
            }
            _ => {}
        }
    }

    if !found {
        anyhow::bail!("Folder {} not found", folder_id);
    }

    // Delete by calling UpdateDialogFilter with filter: None
    let delete_request = tl::functions::messages::UpdateDialogFilter {
        id: folder_id,
        filter: None,
    };

    app.tg.client.invoke(&delete_request).await?;

    if cli.json {
        out::write_json(&serde_json::json!({
            "success": true,
            "id": folder_id,
            "name": folder_name,
        }))?;
    } else {
        println!("Deleted folder '{}' (ID {})", folder_name, folder_id);
    }

    Ok(())
}

async fn show_folder(cli: &Cli, folder_id: i32) -> Result<()> {
    let app = App::new(cli).await?;

    // Get folder filters
    let request = tl::functions::messages::GetDialogFilters {};
    let result = app.tg.client.invoke(&request).await?;

    let filters = match result {
        tl::enums::messages::DialogFilters::Filters(f) => f.filters,
    };

    // Find the requested folder
    let mut folder_filter: Option<(
        &Vec<tl::enums::InputPeer>,
        &Vec<tl::enums::InputPeer>,
        String,
    )> = None;

    for filter_enum in &filters {
        match filter_enum {
            tl::enums::DialogFilter::Filter(f) if f.id == folder_id => {
                let title = match &f.title {
                    tl::enums::TextWithEntities::Entities(t) => t.text.clone(),
                };
                folder_filter = Some((&f.pinned_peers, &f.include_peers, title));
                break;
            }
            tl::enums::DialogFilter::Chatlist(c) if c.id == folder_id => {
                let title = match &c.title {
                    tl::enums::TextWithEntities::Entities(t) => t.text.clone(),
                };
                folder_filter = Some((&c.pinned_peers, &c.include_peers, title));
                break;
            }
            _ => {}
        }
    }

    let (pinned_peers, include_peers, title) =
        folder_filter.ok_or_else(|| anyhow::anyhow!("Folder {} not found", folder_id))?;

    // Collect peer IDs
    let mut chats: Vec<FolderChat> = Vec::new();

    // Process pinned peers
    for peer in pinned_peers {
        if let Some(chat) = resolve_peer_to_chat(&app, peer, true).await? {
            chats.push(chat);
        }
    }

    // Process include peers
    for peer in include_peers {
        if let Some(chat) = resolve_peer_to_chat(&app, peer, false).await? {
            chats.push(chat);
        }
    }

    if cli.json {
        out::write_json(&serde_json::json!({
            "folder_id": folder_id,
            "title": title,
            "chats": chats,
        }))?;
    } else {
        println!("Folder: {} (ID: {})\n", title, folder_id);
        println!("{:<16} {:<30} {:<10} PINNED", "ID", "NAME", "KIND");
        for c in &chats {
            println!(
                "{:<16} {:<30} {:<10} {}",
                c.id,
                out::truncate(&c.name, 28),
                c.kind,
                if c.pinned { "yes" } else { "" }
            );
        }
    }

    Ok(())
}

async fn resolve_peer_to_chat(
    app: &App,
    peer: &tl::enums::InputPeer,
    pinned: bool,
) -> Result<Option<FolderChat>> {
    let (id, kind) = match peer {
        tl::enums::InputPeer::User(u) => (u.user_id, "user"),
        tl::enums::InputPeer::Chat(c) => (c.chat_id, "group"),
        tl::enums::InputPeer::Channel(c) => (c.channel_id, "channel"),
        tl::enums::InputPeer::UserFromMessage(_)
        | tl::enums::InputPeer::ChannelFromMessage(_)
        | tl::enums::InputPeer::PeerSelf
        | tl::enums::InputPeer::Empty => return Ok(None),
    };

    // Try to get name from local store
    let name = if let Some(chat) = app.store.get_chat(id).await? {
        chat.name
    } else {
        format!("ID:{}", id)
    };

    Ok(Some(FolderChat {
        id,
        name,
        kind: kind.to_string(),
        pinned,
    }))
}

async fn add_to_folder(cli: &Cli, chat_id: i64, folder_id: i32) -> Result<()> {
    let app = App::new(cli).await?;

    // Get current folder filters
    let request = tl::functions::messages::GetDialogFilters {};
    let result = app.tg.client.invoke(&request).await?;

    let filters = match result {
        tl::enums::messages::DialogFilters::Filters(f) => f.filters,
    };

    // Find the folder and create updated version
    let mut found = false;
    for filter_enum in &filters {
        match filter_enum {
            tl::enums::DialogFilter::Filter(f) if f.id == folder_id => {
                found = true;

                // Resolve chat to InputPeer
                let input_peer = resolve_chat_to_input_peer(&app, chat_id).await?;

                // Check if already in folder
                let already_in = f.include_peers.iter().any(|p| peer_matches(p, chat_id))
                    || f.pinned_peers.iter().any(|p| peer_matches(p, chat_id));

                if already_in {
                    if cli.json {
                        out::write_json(&serde_json::json!({
                            "success": true,
                            "chat_id": chat_id,
                            "folder_id": folder_id,
                            "message": "Chat already in folder"
                        }))?;
                    } else {
                        println!("Chat {} is already in folder {}", chat_id, folder_id);
                    }
                    return Ok(());
                }

                // Create new include_peers with the chat added
                let mut new_include_peers = f.include_peers.clone();
                new_include_peers.push(input_peer);

                // Update filter
                let updated = tl::types::DialogFilter {
                    contacts: f.contacts,
                    non_contacts: f.non_contacts,
                    groups: f.groups,
                    broadcasts: f.broadcasts,
                    bots: f.bots,
                    exclude_muted: f.exclude_muted,
                    exclude_read: f.exclude_read,
                    exclude_archived: f.exclude_archived,
                    title_noanimate: f.title_noanimate,
                    id: f.id,
                    title: f.title.clone(),
                    emoticon: f.emoticon.clone(),
                    color: f.color,
                    pinned_peers: f.pinned_peers.clone(),
                    include_peers: new_include_peers,
                    exclude_peers: f.exclude_peers.clone(),
                };

                let update_request = tl::functions::messages::UpdateDialogFilter {
                    id: folder_id,
                    filter: Some(tl::enums::DialogFilter::Filter(updated)),
                };

                app.tg.client.invoke(&update_request).await?;
                break;
            }
            tl::enums::DialogFilter::Chatlist(c) if c.id == folder_id => {
                found = true;

                // Resolve chat to InputPeer
                let input_peer = resolve_chat_to_input_peer(&app, chat_id).await?;

                // Check if already in folder
                let already_in = c.include_peers.iter().any(|p| peer_matches(p, chat_id))
                    || c.pinned_peers.iter().any(|p| peer_matches(p, chat_id));

                if already_in {
                    if cli.json {
                        out::write_json(&serde_json::json!({
                            "success": true,
                            "chat_id": chat_id,
                            "folder_id": folder_id,
                            "message": "Chat already in folder"
                        }))?;
                    } else {
                        println!("Chat {} is already in folder {}", chat_id, folder_id);
                    }
                    return Ok(());
                }

                // Create new include_peers with the chat added
                let mut new_include_peers = c.include_peers.clone();
                new_include_peers.push(input_peer);

                // Update filter as chatlist
                let updated = tl::types::DialogFilterChatlist {
                    has_my_invites: c.has_my_invites,
                    title_noanimate: c.title_noanimate,
                    id: c.id,
                    title: c.title.clone(),
                    emoticon: c.emoticon.clone(),
                    color: c.color,
                    pinned_peers: c.pinned_peers.clone(),
                    include_peers: new_include_peers,
                };

                let update_request = tl::functions::messages::UpdateDialogFilter {
                    id: folder_id,
                    filter: Some(tl::enums::DialogFilter::Chatlist(updated)),
                };

                app.tg.client.invoke(&update_request).await?;
                break;
            }
            _ => {}
        }
    }

    if !found {
        anyhow::bail!("Folder {} not found", folder_id);
    }

    if cli.json {
        out::write_json(&serde_json::json!({
            "success": true,
            "chat_id": chat_id,
            "folder_id": folder_id,
        }))?;
    } else {
        println!("Added chat {} to folder {}", chat_id, folder_id);
    }

    Ok(())
}

async fn remove_from_folder(cli: &Cli, chat_id: i64, folder_id: i32) -> Result<()> {
    let app = App::new(cli).await?;

    // Get current folder filters
    let request = tl::functions::messages::GetDialogFilters {};
    let result = app.tg.client.invoke(&request).await?;

    let filters = match result {
        tl::enums::messages::DialogFilters::Filters(f) => f.filters,
    };

    // Find the folder and create updated version
    let mut found = false;
    for filter_enum in &filters {
        match filter_enum {
            tl::enums::DialogFilter::Filter(f) if f.id == folder_id => {
                found = true;

                // Remove from include_peers and pinned_peers
                let new_include_peers: Vec<_> = f
                    .include_peers
                    .iter()
                    .filter(|p| !peer_matches(p, chat_id))
                    .cloned()
                    .collect();
                let new_pinned_peers: Vec<_> = f
                    .pinned_peers
                    .iter()
                    .filter(|p| !peer_matches(p, chat_id))
                    .cloned()
                    .collect();

                let was_removed = new_include_peers.len() < f.include_peers.len()
                    || new_pinned_peers.len() < f.pinned_peers.len();

                if !was_removed {
                    if cli.json {
                        out::write_json(&serde_json::json!({
                            "success": true,
                            "chat_id": chat_id,
                            "folder_id": folder_id,
                            "message": "Chat not in folder"
                        }))?;
                    } else {
                        println!("Chat {} is not in folder {}", chat_id, folder_id);
                    }
                    return Ok(());
                }

                // Update filter
                let updated = tl::types::DialogFilter {
                    contacts: f.contacts,
                    non_contacts: f.non_contacts,
                    groups: f.groups,
                    broadcasts: f.broadcasts,
                    bots: f.bots,
                    exclude_muted: f.exclude_muted,
                    exclude_read: f.exclude_read,
                    exclude_archived: f.exclude_archived,
                    title_noanimate: f.title_noanimate,
                    id: f.id,
                    title: f.title.clone(),
                    emoticon: f.emoticon.clone(),
                    color: f.color,
                    pinned_peers: new_pinned_peers,
                    include_peers: new_include_peers,
                    exclude_peers: f.exclude_peers.clone(),
                };

                let update_request = tl::functions::messages::UpdateDialogFilter {
                    id: folder_id,
                    filter: Some(tl::enums::DialogFilter::Filter(updated)),
                };

                app.tg.client.invoke(&update_request).await?;
                break;
            }
            tl::enums::DialogFilter::Chatlist(c) if c.id == folder_id => {
                found = true;

                // Remove from include_peers and pinned_peers
                let new_include_peers: Vec<_> = c
                    .include_peers
                    .iter()
                    .filter(|p| !peer_matches(p, chat_id))
                    .cloned()
                    .collect();
                let new_pinned_peers: Vec<_> = c
                    .pinned_peers
                    .iter()
                    .filter(|p| !peer_matches(p, chat_id))
                    .cloned()
                    .collect();

                let was_removed = new_include_peers.len() < c.include_peers.len()
                    || new_pinned_peers.len() < c.pinned_peers.len();

                if !was_removed {
                    if cli.json {
                        out::write_json(&serde_json::json!({
                            "success": true,
                            "chat_id": chat_id,
                            "folder_id": folder_id,
                            "message": "Chat not in folder"
                        }))?;
                    } else {
                        println!("Chat {} is not in folder {}", chat_id, folder_id);
                    }
                    return Ok(());
                }

                // Update filter as chatlist
                let updated = tl::types::DialogFilterChatlist {
                    has_my_invites: c.has_my_invites,
                    title_noanimate: c.title_noanimate,
                    id: c.id,
                    title: c.title.clone(),
                    emoticon: c.emoticon.clone(),
                    color: c.color,
                    pinned_peers: new_pinned_peers,
                    include_peers: new_include_peers,
                };

                let update_request = tl::functions::messages::UpdateDialogFilter {
                    id: folder_id,
                    filter: Some(tl::enums::DialogFilter::Chatlist(updated)),
                };

                app.tg.client.invoke(&update_request).await?;
                break;
            }
            _ => {}
        }
    }

    if !found {
        anyhow::bail!("Folder {} not found", folder_id);
    }

    if cli.json {
        out::write_json(&serde_json::json!({
            "success": true,
            "chat_id": chat_id,
            "folder_id": folder_id,
        }))?;
    } else {
        println!("Removed chat {} from folder {}", chat_id, folder_id);
    }

    Ok(())
}

/// Resolve a chat ID to an InputPeer by iterating dialogs
async fn resolve_chat_to_input_peer(app: &App, chat_id: i64) -> Result<tl::enums::InputPeer> {
    // First check local store for chat info
    let chat = app.store.get_chat(chat_id).await?;

    // Try to find via session
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

    // If we have the chat info, try to resolve via dialogs
    if chat.is_some() {
        let mut dialogs = app.tg.client.iter_dialogs();
        while let Some(dialog) = dialogs.next().await? {
            let peer = dialog.peer();
            if peer.id().bare_id() == chat_id {
                return Ok(PeerRef::from(peer).into());
            }
        }
    }

    anyhow::bail!(
        "Could not resolve chat {}. Make sure you've synced first.",
        chat_id
    );
}

/// Check if an InputPeer matches a chat ID
fn peer_matches(peer: &tl::enums::InputPeer, chat_id: i64) -> bool {
    match peer {
        tl::enums::InputPeer::User(u) => u.user_id == chat_id,
        tl::enums::InputPeer::Chat(c) => c.chat_id == chat_id,
        tl::enums::InputPeer::Channel(c) => c.channel_id == chat_id,
        _ => false,
    }
}
