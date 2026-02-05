use crate::app::App;
use crate::store::UpsertMessageParams;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use grammers_client::types::{Media, Message as TgMessage, Peer};
use grammers_session::defs::PeerRef;
use grammers_tl_types as tl;
use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

/// Maximum messages to fetch per chat during incremental sync (effectively unlimited).
const INCREMENTAL_MAX_MESSAGES: usize = 10000;

#[derive(Debug, Clone, Copy)]
pub enum SyncMode {
    Once,
}

#[derive(Debug, Clone, Copy)]
pub enum OutputMode {
    None,
    Text,
    Json,
    /// JSONL streaming (one JSON object per line, flushed immediately)
    Stream,
}

pub struct SyncOptions {
    pub mode: SyncMode,
    pub output: OutputMode,
    pub mark_read: bool,
    pub download_media: bool,
    pub enable_socket: bool,
    pub idle_exit_secs: u64,
    pub ignore_chat_ids: Vec<i64>,
    pub ignore_channels: bool,
    pub show_progress: bool,
    pub incremental: bool,
    pub messages_per_chat: usize,
}

/// Get media type string and file extension from grammers Media enum
fn media_info(media: &Media) -> (String, String) {
    match media {
        Media::Photo(_) => ("photo".to_string(), "jpg".to_string()),
        Media::Document(doc) => {
            let media_type = if doc.duration().is_some() {
                if doc.resolution().is_some() {
                    "video"
                } else {
                    "audio"
                }
            } else {
                "document"
            };

            // Try to get extension from filename first
            let ext = if let Some(name) = Some(doc.name()).filter(|n| !n.is_empty()) {
                Path::new(name)
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|s| s.to_lowercase())
            } else {
                None
            };

            // Fall back to mime type
            let ext = ext.unwrap_or_else(|| {
                doc.mime_type()
                    .map(mime_to_ext)
                    .unwrap_or_else(|| "bin".to_string())
            });

            (media_type.to_string(), ext)
        }
        Media::Sticker(sticker) => {
            let ext = if sticker.is_animated() {
                "tgs".to_string()
            } else {
                sticker
                    .document
                    .mime_type()
                    .map(mime_to_ext)
                    .unwrap_or_else(|| "webp".to_string())
            };
            ("sticker".to_string(), ext)
        }
        Media::Contact(_) => ("contact".to_string(), "vcf".to_string()),
        Media::Poll(_) => ("poll".to_string(), "".to_string()),
        Media::Geo(_) => ("geo".to_string(), "".to_string()),
        Media::Dice(_) => ("dice".to_string(), "".to_string()),
        Media::Venue(_) => ("venue".to_string(), "".to_string()),
        Media::GeoLive(_) => ("geolive".to_string(), "".to_string()),
        Media::WebPage(_) => ("webpage".to_string(), "".to_string()),
        _ => ("media".to_string(), "bin".to_string()),
    }
}

/// Convert MIME type to file extension
fn mime_to_ext(mime: &str) -> String {
    match mime {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/quicktime" => "mov",
        "audio/ogg" | "audio/opus" => "ogg",
        "audio/mpeg" | "audio/mp3" => "mp3",
        "audio/mp4" | "audio/m4a" => "m4a",
        "audio/wav" => "wav",
        "audio/flac" => "flac",
        "application/pdf" => "pdf",
        "application/zip" => "zip",
        "application/x-rar-compressed" => "rar",
        "application/x-7z-compressed" => "7z",
        "text/plain" => "txt",
        "application/json" => "json",
        "application/x-tgsticker" => "tgs",
        _ => mime.split('/').next_back().unwrap_or("bin"),
    }
    .to_string()
}

pub struct SyncResult {
    pub messages_stored: u64,
    pub chats_stored: u64,
}

impl App {
    /// Download media from a message if present and return (media_type, media_path)
    async fn download_message_media(
        &self,
        msg: &TgMessage,
        chat_id: i64,
    ) -> Result<(Option<String>, Option<String>)> {
        let media = match msg.media() {
            Some(m) => m,
            None => return Ok((None, None)),
        };

        let (media_type, ext) = media_info(&media);

        // Skip non-downloadable media types
        if ext.is_empty() {
            return Ok((Some(media_type), None));
        }

        // Build path: {store_dir}/media/{chat_id}/{message_id}.{ext}
        let media_dir = Path::new(&self.store_dir)
            .join("media")
            .join(chat_id.to_string());

        // Create directory if needed
        std::fs::create_dir_all(&media_dir)?;

        let file_name = format!("{}.{}", msg.id(), ext);
        let file_path = media_dir.join(&file_name);

        // Skip if file already exists (idempotent)
        if file_path.exists() {
            return Ok((
                Some(media_type),
                Some(file_path.to_string_lossy().to_string()),
            ));
        }

        // Download the media
        match self.tg.client.download_media(&media, &file_path).await {
            Ok(()) => {
                log::info!(
                    "Downloaded media: chat={} msg={} -> {}",
                    chat_id,
                    msg.id(),
                    file_path.display()
                );
                Ok((
                    Some(media_type),
                    Some(file_path.to_string_lossy().to_string()),
                ))
            }
            Err(e) => {
                log::warn!(
                    "Failed to download media for chat={} msg={}: {}",
                    chat_id,
                    msg.id(),
                    e
                );
                // Return media type but no path on failure
                Ok((Some(media_type), None))
            }
        }
    }

    pub async fn sync(&mut self, opts: SyncOptions) -> Result<SyncResult> {
        let mut messages_stored: u64 = 0;
        let mut chats_stored: u64 = 0;

        // Build ignore set for fast lookup.
        let ignore_set: HashSet<i64> = opts.ignore_chat_ids.iter().copied().collect();

        // Helper to check if a chat should be ignored.
        let should_ignore = |chat_id: i64, kind: &str| -> bool {
            if ignore_set.contains(&chat_id) {
                return true;
            }
            if opts.ignore_channels && kind == "channel" {
                return true;
            }
            false
        };

        let client = &self.tg.client;

        // Progress tracking
        let mut last_progress_time = std::time::Instant::now();
        let progress_interval = Duration::from_millis(500);

        // Phase 1: Bootstrap — fetch recent dialogs and their messages
        if opts.show_progress {
            eprint!("\rSyncing... 0 chats, 0 messages");
        }
        let mut dialogs = client.iter_dialogs();
        while let Some(dialog) = dialogs
            .next()
            .await
            .context("Failed to fetch dialogs from Telegram")?
        {
            let peer = dialog.peer();
            let (kind, name, username, is_forum) = peer_info(peer);
            let id = peer.id().bare_id();

            // Skip ignored chats.
            if should_ignore(id, &kind) {
                continue;
            }

            self.store
                .upsert_chat(id, &kind, &name, username.as_deref(), None, is_forum)
                .await?;
            chats_stored += 1;

            // Also store as contact if it's a user
            if let Peer::User(ref user) = peer {
                self.store
                    .upsert_contact(
                        user.bare_id(),
                        user.username(),
                        user.first_name().unwrap_or(""),
                        user.last_name().unwrap_or(""),
                        user.phone().unwrap_or(""),
                    )
                    .await?;
            }

            // Fetch messages for this chat
            let peer_ref = PeerRef::from(peer);
            let mut message_iter = client.iter_messages(peer_ref);
            let mut count = 0;
            let mut latest_ts: Option<DateTime<Utc>> = None;
            let mut highest_msg_id: Option<i64> = None;

            // For incremental sync, get the last synced message ID
            let last_sync_id = if opts.incremental {
                self.store.get_last_sync_message_id(id).await.ok().flatten()
            } else {
                None
            };

            // Determine max messages to fetch
            let max_messages = if opts.incremental && last_sync_id.is_some() {
                INCREMENTAL_MAX_MESSAGES
            } else {
                opts.messages_per_chat
            };

            while let Some(msg) = message_iter
                .next()
                .await
                .with_context(|| format!("Failed to fetch messages for chat {} ({})", name, id))?
            {
                let msg_id = msg.id() as i64;

                // For incremental sync, stop when we hit a message we've already seen
                if let Some(last_id) = last_sync_id {
                    if msg_id <= last_id {
                        log::debug!(
                            "Chat {}: reached last synced message {} (stopping at {})",
                            id,
                            last_id,
                            msg_id
                        );
                        break;
                    }
                }

                if count >= max_messages {
                    break;
                }
                count += 1;

                // Track the highest message ID we've seen
                if highest_msg_id.is_none() || msg_id > highest_msg_id.unwrap() {
                    highest_msg_id = Some(msg_id);
                }

                let msg_ts = msg.date();
                if latest_ts.is_none() || msg_ts > latest_ts.unwrap() {
                    latest_ts = Some(msg_ts);
                }

                let sender_id = msg.sender().map(|s| s.id().bare_id()).unwrap_or(0);
                let from_me = msg.outgoing();

                let text = msg.text().to_string();
                let reply_to_id = msg.reply_to_message_id().map(|id| id as i64);
                let topic_id = if is_forum {
                    extract_topic_id(&msg)
                } else {
                    None
                };

                // Download media if enabled
                let (media_type, media_path) = if opts.download_media {
                    self.download_message_media(&msg, id).await?
                } else {
                    (msg.media().map(|_| "media".to_string()), None)
                };

                // Clone media_type for use in stream output after the move
                let media_type_out = media_type.clone();

                self.store
                    .upsert_message(UpsertMessageParams {
                        id: msg.id() as i64,
                        chat_id: id,
                        sender_id,
                        ts: msg_ts,
                        edit_ts: msg.edit_date(),
                        from_me,
                        text: text.clone(),
                        media_type,
                        media_path,
                        reply_to_id,
                        topic_id,
                    })
                    .await?;
                messages_stored += 1;

                // Show progress periodically
                if opts.show_progress && last_progress_time.elapsed() >= progress_interval {
                    eprint!(
                        "\rSyncing... {} chats, {} messages",
                        chats_stored, messages_stored
                    );
                    last_progress_time = std::time::Instant::now();
                }

                // Output
                match opts.output {
                    OutputMode::Text => {
                        let from_label = if from_me {
                            "me".to_string()
                        } else {
                            sender_id.to_string()
                        };
                        let short_text = text.replace('\n', " ");
                        let short_text = if short_text.len() > 100 {
                            // Find the last valid char boundary at or before byte 100
                            let truncate_at = short_text
                                .char_indices()
                                .take_while(|(i, _)| *i < 100)
                                .last()
                                .map(|(i, c)| i + c.len_utf8())
                                .unwrap_or(0);
                            format!("{}…", &short_text[..truncate_at])
                        } else {
                            short_text
                        };
                        println!(
                            "from={} chat={} id={} text={}",
                            from_label,
                            id,
                            msg.id(),
                            short_text
                        );
                    }
                    OutputMode::Json => {
                        let obj = serde_json::json!({
                            "from_me": from_me,
                            "sender": sender_id,
                            "chat": id,
                            "id": msg.id(),
                            "timestamp": msg_ts.to_rfc3339(),
                            "text": text,
                        });
                        println!("{}", serde_json::to_string(&obj).unwrap_or_default());
                    }
                    OutputMode::Stream => {
                        use std::io::Write;
                        let obj = serde_json::json!({
                            "type": "message",
                            "from_me": from_me,
                            "sender_id": sender_id,
                            "chat_id": id,
                            "id": msg.id(),
                            "ts": msg_ts.to_rfc3339(),
                            "text": text,
                            "topic_id": topic_id,
                            "media_type": media_type_out,
                        });
                        println!("{}", serde_json::to_string(&obj).unwrap_or_default());
                        let _ = std::io::stdout().flush();
                    }
                    OutputMode::None => {}
                }
            }

            // Update chat's last_message_ts
            if let Some(ts) = latest_ts {
                self.store
                    .upsert_chat(id, &kind, &name, username.as_deref(), Some(ts), is_forum)
                    .await?;
            }

            // Update last_sync_message_id for incremental sync
            if let Some(high_id) = highest_msg_id {
                self.store.update_last_sync_message_id(id, high_id).await?;
            }

            // If it's a forum, sync topics
            if is_forum {
                if let Ok(topic_count) = self.sync_topics(id).await {
                    log::info!("Synced {} topics for forum chat {}", topic_count, id);
                }
            }
        }

        // Phase 1b: Also sync archived dialogs (folder_id=1)
        if opts.show_progress {
            eprint!(
                "\rSyncing archived... {} chats, {} messages",
                chats_stored, messages_stored
            );
        }

        let archived_peers = self.fetch_archived_dialogs().await?;
        for peer in archived_peers {
            let (kind, name, username, is_forum) = peer_info(&peer);
            let id = peer.id().bare_id();

            // Skip ignored chats.
            if should_ignore(id, &kind) {
                continue;
            }

            self.store
                .upsert_chat(id, &kind, &name, username.as_deref(), None, is_forum)
                .await?;
            chats_stored += 1;

            // Also store as contact if it's a user
            if let Peer::User(ref user) = peer {
                self.store
                    .upsert_contact(
                        user.bare_id(),
                        user.username(),
                        user.first_name().unwrap_or(""),
                        user.last_name().unwrap_or(""),
                        user.phone().unwrap_or(""),
                    )
                    .await?;
            }

            // Fetch messages for this chat
            let peer_ref = PeerRef::from(&peer);
            let mut message_iter = client.iter_messages(peer_ref);
            let mut count = 0;
            let mut latest_ts: Option<DateTime<Utc>> = None;
            let mut highest_msg_id: Option<i64> = None;

            // For incremental sync, get the last synced message ID
            let last_sync_id = if opts.incremental {
                self.store.get_last_sync_message_id(id).await.ok().flatten()
            } else {
                None
            };

            // Determine max messages to fetch
            let max_messages = if opts.incremental && last_sync_id.is_some() {
                INCREMENTAL_MAX_MESSAGES
            } else {
                opts.messages_per_chat
            };

            while let Some(msg) = message_iter.next().await.with_context(|| {
                format!(
                    "Failed to fetch messages for archived chat {} ({})",
                    name, id
                )
            })? {
                let msg_id = msg.id() as i64;

                // For incremental sync, stop when we hit a message we've already seen
                if let Some(last_id) = last_sync_id {
                    if msg_id <= last_id {
                        log::debug!(
                            "Archived chat {}: reached last synced message {} (stopping at {})",
                            id,
                            last_id,
                            msg_id
                        );
                        break;
                    }
                }

                if count >= max_messages {
                    break;
                }
                count += 1;

                // Track the highest message ID we've seen
                if highest_msg_id.is_none() || msg_id > highest_msg_id.unwrap() {
                    highest_msg_id = Some(msg_id);
                }

                let msg_ts = msg.date();
                if latest_ts.is_none() || msg_ts > latest_ts.unwrap() {
                    latest_ts = Some(msg_ts);
                }

                let sender_id = msg.sender().map(|s| s.id().bare_id()).unwrap_or(0);
                let from_me = msg.outgoing();

                let text = msg.text().to_string();
                let reply_to_id = msg.reply_to_message_id().map(|id| id as i64);
                let topic_id = if is_forum {
                    extract_topic_id(&msg)
                } else {
                    None
                };

                // Download media if enabled
                let (media_type, media_path) = if opts.download_media {
                    self.download_message_media(&msg, id).await?
                } else {
                    (msg.media().map(|_| "media".to_string()), None)
                };

                self.store
                    .upsert_message(UpsertMessageParams {
                        id: msg.id() as i64,
                        chat_id: id,
                        sender_id,
                        ts: msg_ts,
                        edit_ts: msg.edit_date(),
                        from_me,
                        text: text.clone(),
                        media_type,
                        media_path,
                        reply_to_id,
                        topic_id,
                    })
                    .await?;
                messages_stored += 1;

                // Show progress periodically
                if opts.show_progress && last_progress_time.elapsed() >= progress_interval {
                    eprint!(
                        "\rSyncing archived... {} chats, {} messages",
                        chats_stored, messages_stored
                    );
                    last_progress_time = std::time::Instant::now();
                }
            }

            // Update chat's last_message_ts
            if let Some(ts) = latest_ts {
                self.store
                    .upsert_chat(id, &kind, &name, username.as_deref(), Some(ts), is_forum)
                    .await?;
            }

            // Update last_sync_message_id for incremental sync
            if let Some(high_id) = highest_msg_id {
                self.store.update_last_sync_message_id(id, high_id).await?;
            }

            // If it's a forum, sync topics
            if is_forum {
                if let Ok(topic_count) = self.sync_topics(id).await {
                    log::info!(
                        "Synced {} topics for archived forum chat {}",
                        topic_count,
                        id
                    );
                }
            }
        }

        if opts.show_progress {
            // Clear progress line and print final status
            eprint!("\r\x1b[K"); // Clear line
        }
        eprintln!(
            "Sync complete: {} chats, {} messages",
            chats_stored, messages_stored
        );

        Ok(SyncResult {
            messages_stored,
            chats_stored,
        })
    }

    /// Re-sync dialogs from Telegram (used by socket RPC).
    /// Returns counts of chats and messages synced.
    pub async fn sync_dialogs(
        &self,
        messages_per_chat: usize,
    ) -> Result<crate::app::socket::SyncResult> {
        let mut messages_stored: u64 = 0;
        let mut chats_stored: u64 = 0;

        let client = &self.tg.client;

        let mut dialogs = client.iter_dialogs();
        while let Some(dialog) = dialogs
            .next()
            .await
            .context("Failed to fetch dialogs from Telegram")?
        {
            let peer = dialog.peer();
            let (kind, name, username, is_forum) = peer_info(peer);
            let id = peer.id().bare_id();

            self.store
                .upsert_chat(id, &kind, &name, username.as_deref(), None, is_forum)
                .await?;
            chats_stored += 1;

            // Also store as contact if it's a user
            if let Peer::User(ref user) = peer {
                self.store
                    .upsert_contact(
                        user.bare_id(),
                        user.username(),
                        user.first_name().unwrap_or(""),
                        user.last_name().unwrap_or(""),
                        user.phone().unwrap_or(""),
                    )
                    .await?;
            }

            // Fetch recent messages for this chat
            let peer_ref = PeerRef::from(peer);
            let mut message_iter = client.iter_messages(peer_ref);
            let mut count = 0;
            let mut latest_ts: Option<DateTime<Utc>> = None;

            while let Some(msg) = message_iter
                .next()
                .await
                .with_context(|| format!("Failed to fetch messages for chat {} ({})", name, id))?
            {
                if count >= messages_per_chat {
                    break;
                }
                count += 1;

                let msg_ts = msg.date();
                if latest_ts.is_none() || msg_ts > latest_ts.unwrap() {
                    latest_ts = Some(msg_ts);
                }

                let sender_id = msg.sender().map(|s| s.id().bare_id()).unwrap_or(0);
                let from_me = msg.outgoing();
                let text = msg.text().to_string();
                let reply_to_id = msg.reply_to_message_id().map(|id| id as i64);
                let topic_id = if is_forum {
                    extract_topic_id(&msg)
                } else {
                    None
                };
                let media_type = msg.media().map(|_| "media".to_string());

                self.store
                    .upsert_message(UpsertMessageParams {
                        id: msg.id() as i64,
                        chat_id: id,
                        sender_id,
                        ts: msg_ts,
                        edit_ts: msg.edit_date(),
                        from_me,
                        text,
                        media_type,
                        media_path: None,
                        reply_to_id,
                        topic_id,
                    })
                    .await?;
                messages_stored += 1;
            }

            // Update chat's last_message_ts
            if let Some(ts) = latest_ts {
                self.store
                    .upsert_chat(id, &kind, &name, username.as_deref(), Some(ts), is_forum)
                    .await?;
            }

            // If it's a forum, sync topics
            if is_forum {
                if let Ok(topic_count) = self.sync_topics(id).await {
                    log::info!("Synced {} topics for forum chat {}", topic_count, id);
                }
            }
        }

        log::info!(
            "Socket sync complete: {} chats, {} messages",
            chats_stored,
            messages_stored
        );

        Ok(crate::app::socket::SyncResult {
            chats: chats_stored,
            messages: messages_stored,
        })
    }

    /// Fetch archived dialogs (folder_id=1) using raw API.
    /// Returns a Vec of Peer objects (resolved from users/chats).
    async fn fetch_archived_dialogs(&self) -> Result<Vec<Peer>> {
        let mut all_peers = Vec::new();
        let mut offset_date = 0i32;
        let mut offset_id = 0i32;
        let mut offset_peer = tl::enums::InputPeer::Empty;

        loop {
            let request = tl::functions::messages::GetDialogs {
                exclude_pinned: false,
                folder_id: Some(1), // 1 = Archive folder
                offset_date,
                offset_id,
                offset_peer: offset_peer.clone(),
                limit: 100,
                hash: 0,
            };

            let response = self
                .tg
                .client
                .invoke(&request)
                .await
                .context("Failed to fetch archived dialogs")?;

            let (dialogs, messages, users, chats, is_slice) = match response {
                tl::enums::messages::Dialogs::Dialogs(d) => {
                    (d.dialogs, d.messages, d.users, d.chats, false)
                }
                tl::enums::messages::Dialogs::Slice(d) => {
                    (d.dialogs, d.messages, d.users, d.chats, true)
                }
                tl::enums::messages::Dialogs::NotModified(_) => {
                    break; // No changes
                }
            };

            if dialogs.is_empty() {
                break;
            }

            let batch_count = dialogs.len();

            // Build lookup maps for users and chats
            let users_map: std::collections::HashMap<i64, tl::enums::User> = users
                .into_iter()
                .filter_map(|u| {
                    if let tl::enums::User::User(ref user) = u {
                        Some((user.id, u))
                    } else {
                        None
                    }
                })
                .collect();

            let chats_map: std::collections::HashMap<i64, tl::enums::Chat> = chats
                .into_iter()
                .map(|c| {
                    let id = match &c {
                        tl::enums::Chat::Empty(ch) => ch.id,
                        tl::enums::Chat::Chat(ch) => ch.id,
                        tl::enums::Chat::Forbidden(ch) => ch.id,
                        tl::enums::Chat::Channel(ch) => ch.id,
                        tl::enums::Chat::ChannelForbidden(ch) => ch.id,
                    };
                    (id, c)
                })
                .collect();

            // Track last message info for pagination
            let mut last_offset_date = 0i32;
            let mut last_offset_id = 0i32;
            let mut last_peer_id: Option<i64> = None;
            let mut last_peer_kind: Option<&str> = None;

            for dialog in &dialogs {
                let peer_tl = match dialog {
                    tl::enums::Dialog::Dialog(d) => &d.peer,
                    tl::enums::Dialog::Folder(_) => continue, // Skip folder entries
                };

                // Get top_message for offset tracking
                if let tl::enums::Dialog::Dialog(d) = dialog {
                    for msg in &messages {
                        if let tl::enums::Message::Message(m) = msg {
                            if m.id == d.top_message {
                                last_offset_date = m.date;
                                last_offset_id = m.id;
                                break;
                            }
                        }
                    }
                }

                let peer = match peer_tl {
                    tl::enums::Peer::User(u) => {
                        last_peer_id = Some(u.user_id);
                        last_peer_kind = Some("user");
                        if let Some(user) = users_map.get(&u.user_id) {
                            Peer::User(grammers_client::types::User::from_raw(user.clone()))
                        } else {
                            continue;
                        }
                    }
                    tl::enums::Peer::Chat(c) => {
                        last_peer_id = Some(c.chat_id);
                        last_peer_kind = Some("chat");
                        if let Some(chat) = chats_map.get(&c.chat_id) {
                            Peer::Group(grammers_client::types::Group::from_raw(chat.clone()))
                        } else {
                            continue;
                        }
                    }
                    tl::enums::Peer::Channel(c) => {
                        last_peer_id = Some(c.channel_id);
                        last_peer_kind = Some("channel");
                        if let Some(chat) = chats_map.get(&c.channel_id) {
                            match chat {
                                tl::enums::Chat::Channel(ch) if ch.broadcast => Peer::Channel(
                                    grammers_client::types::Channel::from_raw(chat.clone()),
                                ),
                                tl::enums::Chat::Channel(_) => {
                                    // Megagroup (supergroup) - treat as Group
                                    Peer::Group(grammers_client::types::Group::from_raw(
                                        chat.clone(),
                                    ))
                                }
                                _ => continue,
                            }
                        } else {
                            continue;
                        }
                    }
                };

                all_peers.push(peer);
            }

            // If not a slice or got fewer than requested, we're done
            if !is_slice || batch_count < 100 {
                break;
            }

            // Update offsets for next iteration
            offset_date = last_offset_date;
            offset_id = last_offset_id;
            if let (Some(id), Some(kind)) = (last_peer_id, last_peer_kind) {
                offset_peer = match kind {
                    "user" => tl::enums::InputPeer::User(tl::types::InputPeerUser {
                        user_id: id,
                        access_hash: 0,
                    }),
                    "chat" => tl::enums::InputPeer::Chat(tl::types::InputPeerChat { chat_id: id }),
                    "channel" => tl::enums::InputPeer::Channel(tl::types::InputPeerChannel {
                        channel_id: id,
                        access_hash: 0,
                    }),
                    _ => break,
                };
            } else {
                break;
            }
        }

        log::info!("Fetched {} archived dialogs", all_peers.len());
        Ok(all_peers)
    }
}

/// Returns (kind, name, username, is_forum)
fn peer_info(peer: &Peer) -> (String, String, Option<String>, bool) {
    match peer {
        Peer::User(user) => {
            let name = user.full_name();
            let username = user.username().map(|s| s.to_string());
            ("user".to_string(), name, username, false)
        }
        Peer::Group(group) => {
            let name = group.title().map(|s| s.to_string()).unwrap_or_default();
            let username = group.username().map(|s| s.to_string());
            // Check if this is a forum group (megagroup with forum flag)
            let is_forum = match &group.raw {
                tl::enums::Chat::Channel(channel) => channel.forum,
                _ => false,
            };
            ("group".to_string(), name, username, is_forum)
        }
        Peer::Channel(channel) => {
            let name = channel.title().to_string();
            let username = channel.username().map(|s| s.to_string());
            ("channel".to_string(), name, username, false)
        }
    }
}

/// Extract topic_id from a message's reply header if it's a forum topic message.
///
/// In forum groups:
/// - `reply_to_top_id` always indicates the topic thread ID (when present)
/// - For direct posts to a topic (not replies), `reply_to_msg_id` is the topic ID
/// - The `forum_topic` flag should be set for forum messages, but we also check
///   `reply_to_top_id` unconditionally as it's specifically for forum threads
fn extract_topic_id(msg: &TgMessage) -> Option<i32> {
    match &msg.raw {
        tl::enums::Message::Message(m) => {
            if let Some(tl::enums::MessageReplyHeader::Header(header)) = &m.reply_to {
                // reply_to_top_id is specifically the forum topic/thread ID - always use it if present
                if let Some(top_id) = header.reply_to_top_id {
                    return Some(top_id);
                }

                // For direct posts to a topic (not replying to a specific message),
                // reply_to_msg_id is the topic ID when forum_topic flag is set
                if header.forum_topic {
                    return header.reply_to_msg_id;
                }

                None
            } else {
                None
            }
        }
        _ => None,
    }
}
