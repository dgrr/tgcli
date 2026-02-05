use crate::app::App;
use crate::store::UpsertMessageParams;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use grammers_client::types::{Media, Message as TgMessage, Peer, Update};
use grammers_client::UpdatesConfiguration;
use grammers_session::defs::PeerRef;
use grammers_tl_types as tl;
use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub enum SyncMode {
    Once,
    Follow,
}

#[derive(Debug, Clone, Copy)]
pub enum OutputMode {
    None,
    Text,
    Json,
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
                if count >= 100 {
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
                    OutputMode::None => {}
                }
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

        if opts.show_progress {
            // Clear progress line and print final status
            eprint!("\r\x1b[K"); // Clear line
        }
        eprintln!(
            "Sync complete: {} chats, {} messages",
            chats_stored, messages_stored
        );

        // Phase 2: Follow mode — listen for updates
        if matches!(opts.mode, SyncMode::Follow) {
            eprintln!("Listening for updates (Ctrl+C to stop)…");

            // Start socket server if requested
            let (socket_cmd_tx, mut socket_cmd_rx) = crate::app::socket::command_channel();
            let _socket_handle = if opts.enable_socket {
                let store_dir = self.store_dir.clone();
                Some(tokio::spawn(async move {
                    if let Err(e) = crate::app::socket::run_server(&store_dir, socket_cmd_tx).await
                    {
                        log::error!("Socket server error: {}", e);
                    }
                }))
            } else {
                // Drop the sender if socket is disabled
                drop(socket_cmd_tx);
                None
            };

            // Take ownership of updates receiver
            if let Some(updates_rx) = self.updates_rx.take() {
                let mut update_stream =
                    client.stream_updates(updates_rx, UpdatesConfiguration::default());

                let idle_duration = Duration::from_secs(opts.idle_exit_secs);
                let mut last_activity = std::time::Instant::now();

                loop {
                    // Use select! to handle both updates and socket commands
                    tokio::select! {
                        // Handle socket commands
                        Some(cmd) = socket_cmd_rx.recv() => {
                            match cmd {
                                crate::app::socket::SocketCommand::Backfill { chat_id, limit, response_tx } => {
                                    log::info!("Socket backfill request: chat_id={}, limit={}", chat_id, limit);
                                    // Get oldest message ID for this chat
                                    let oldest_id = self.store.get_oldest_message_id(chat_id, None).await.ok().flatten();
                                    // Perform backfill using existing logic
                                    let result = self.backfill_messages(chat_id, None, oldest_id, limit).await;
                                    let _ = response_tx.send(result.map_err(|e| e.to_string()));
                                }
                                crate::app::socket::SocketCommand::Read { chat_id, topic_id, all_topics, response_tx } => {
                                    log::info!("Socket read request: chat_id={}, topic_id={:?}, all_topics={}", chat_id, topic_id, all_topics);
                                    let result = if all_topics {
                                        // Mark all topics as read
                                        match self.mark_read_all_topics(chat_id).await {
                                            Ok(count) => Ok(crate::app::socket::ReadResult {
                                                marked_read: true,
                                                topics_count: Some(count),
                                            }),
                                            Err(e) => Err(e.to_string()),
                                        }
                                    } else {
                                        // Mark chat or specific topic as read
                                        match self.mark_read(chat_id, topic_id).await {
                                            Ok(()) => Ok(crate::app::socket::ReadResult {
                                                marked_read: true,
                                                topics_count: None,
                                            }),
                                            Err(e) => Err(e.to_string()),
                                        }
                                    };
                                    let _ = response_tx.send(result);
                                }
                                crate::app::socket::SocketCommand::Sync { limit, response_tx } => {
                                    log::info!("Socket sync request: limit={}", limit);
                                    let result = self.sync_dialogs(limit).await;
                                    let _ = response_tx.send(result.map_err(|e| e.to_string()));
                                }
                                crate::app::socket::SocketCommand::Stop { response_tx } => {
                                    log::info!("Socket stop request received");
                                    let _ = response_tx.send(Ok(()));
                                    break; // Exit the loop to shutdown
                                }
                            }
                        }
                        // Handle idle timeout
                        _ = tokio::time::sleep(idle_duration), if matches!(opts.mode, SyncMode::Once) && last_activity.elapsed() >= idle_duration => {
                            eprintln!("Idle timeout reached, exiting.");
                            break;
                        }
                        // Handle Telegram updates
                        update = update_stream.next() => {
                    match update {
                        Ok(update) => {
                            last_activity = std::time::Instant::now();
                            match update {
                                Update::NewMessage(msg) if !msg.outgoing() => {
                                    let chat_peer = msg.peer();
                                    let (kind, name, username, is_forum) = match &chat_peer {
                                        Ok(p) => peer_info(p),
                                        Err(_) => {
                                            ("unknown".to_string(), "".to_string(), None, false)
                                        }
                                    };
                                    let chat_id = msg.peer_id().bare_id();

                                    // Skip ignored chats.
                                    if should_ignore(chat_id, &kind) {
                                        continue;
                                    }

                                    let sender_id =
                                        msg.sender().map(|s| s.id().bare_id()).unwrap_or(0);
                                    let msg_ts = msg.date();
                                    let text = msg.text().to_string();
                                    let topic_id = if is_forum {
                                        extract_topic_id(&msg)
                                    } else {
                                        None
                                    };

                                    // Download media if enabled
                                    let (media_type, media_path) = if opts.download_media {
                                        self.download_message_media(&msg, chat_id).await?
                                    } else {
                                        (msg.media().map(|_| "media".to_string()), None)
                                    };

                                    self.store
                                        .upsert_chat(
                                            chat_id,
                                            &kind,
                                            &name,
                                            username.as_deref(),
                                            Some(msg_ts),
                                            is_forum,
                                        )
                                        .await?;

                                    self.store
                                        .upsert_message(UpsertMessageParams {
                                            id: msg.id() as i64,
                                            chat_id,
                                            sender_id,
                                            ts: msg_ts,
                                            edit_ts: msg.edit_date(),
                                            from_me: false,
                                            text: text.clone(),
                                            media_type,
                                            media_path,
                                            reply_to_id: msg
                                                .reply_to_message_id()
                                                .map(|id| id as i64),
                                            topic_id,
                                        })
                                        .await?;
                                    messages_stored += 1;

                                    if opts.mark_read {
                                        if let Ok(p) = msg.peer() {
                                            let _ = client.mark_as_read(PeerRef::from(p)).await;
                                        }
                                    }

                                    match opts.output {
                                        OutputMode::Text => {
                                            let short = text.replace('\n', " ");
                                            let short = if short.len() > 100 {
                                                format!("{}…", &short[..100])
                                            } else {
                                                short
                                            };
                                            println!(
                                                "from={} chat={} id={} text={}",
                                                sender_id,
                                                chat_id,
                                                msg.id(),
                                                short
                                            );
                                        }
                                        OutputMode::Json => {
                                            let obj = serde_json::json!({
                                                "from_me": false,
                                                "sender": sender_id,
                                                "chat": chat_id,
                                                "id": msg.id(),
                                                "timestamp": msg_ts.to_rfc3339(),
                                                "text": text,
                                            });
                                            println!(
                                                "{}",
                                                serde_json::to_string(&obj).unwrap_or_default()
                                            );
                                        }
                                        OutputMode::None => {}
                                    }

                                }
                                Update::NewMessage(msg) if msg.outgoing() => {
                                    let chat_id = msg.peer_id().bare_id();
                                    let (kind, _, _, is_forum) = match msg.peer() {
                                        Ok(p) => peer_info(p),
                                        Err(_) => {
                                            ("unknown".to_string(), "".to_string(), None, false)
                                        }
                                    };

                                    // Skip ignored chats.
                                    if should_ignore(chat_id, &kind) {
                                        continue;
                                    }

                                    let msg_ts = msg.date();
                                    let topic_id = if is_forum {
                                        extract_topic_id(&msg)
                                    } else {
                                        None
                                    };

                                    // Download media if enabled
                                    let (media_type, media_path) = if opts.download_media {
                                        self.download_message_media(&msg, chat_id).await?
                                    } else {
                                        (msg.media().map(|_| "media".to_string()), None)
                                    };

                                    self.store
                                        .upsert_message(UpsertMessageParams {
                                            id: msg.id() as i64,
                                            chat_id,
                                            sender_id: 0,
                                            ts: msg_ts,
                                            edit_ts: msg.edit_date(),
                                            from_me: true,
                                            text: msg.text().to_string(),
                                            media_type,
                                            media_path,
                                            reply_to_id: msg
                                                .reply_to_message_id()
                                                .map(|id| id as i64),
                                            topic_id,
                                        })
                                        .await?;
                                    messages_stored += 1;
                                }
                                Update::MessageEdited(msg) => {
                                    let chat_id = msg.peer_id().bare_id();
                                    let (kind, _, _, is_forum) = match msg.peer() {
                                        Ok(p) => peer_info(p),
                                        Err(_) => {
                                            ("unknown".to_string(), "".to_string(), None, false)
                                        }
                                    };

                                    // Skip ignored chats.
                                    if should_ignore(chat_id, &kind) {
                                        continue;
                                    }

                                    let msg_ts = msg.date();
                                    let topic_id = if is_forum {
                                        extract_topic_id(&msg)
                                    } else {
                                        None
                                    };

                                    // Download media if enabled (edits might add media)
                                    let (media_type, media_path) = if opts.download_media {
                                        self.download_message_media(&msg, chat_id).await?
                                    } else {
                                        (msg.media().map(|_| "media".to_string()), None)
                                    };

                                    self.store
                                        .upsert_message(UpsertMessageParams {
                                            id: msg.id() as i64,
                                            chat_id,
                                            sender_id: msg
                                                .sender()
                                                .map(|s| s.id().bare_id())
                                                .unwrap_or(0),
                                            ts: msg_ts,
                                            edit_ts: msg.edit_date(),
                                            from_me: msg.outgoing(),
                                            text: msg.text().to_string(),
                                            media_type,
                                            media_path,
                                            reply_to_id: msg
                                                .reply_to_message_id()
                                                .map(|id| id as i64),
                                            topic_id,
                                        })
                                        .await?;
                                }
                                _ => {}
                            }
                        }
                        Err(e) => {
                            log::error!("Update error: {}", e);
                            break;
                        }
                    }
                        }
                    }
                }
            }
        }

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
