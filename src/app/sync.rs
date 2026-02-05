use crate::app::App;
use crate::store::UpsertMessageParams;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use futures::stream::{self, StreamExt};
use grammers_client::types::{Media, Message as TgMessage, Peer};
use grammers_client::Client;
use grammers_session::defs::{PeerAuth, PeerId, PeerRef};
use grammers_session::storages::SqliteSession;
use grammers_session::Session;
use grammers_tl_types as tl;
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

/// Maximum messages to fetch per chat during incremental sync (effectively unlimited).
const INCREMENTAL_MAX_MESSAGES: usize = 10000;

#[derive(Debug, Clone, Copy)]
pub enum OutputMode {
    None,
    Text,
    Json,
    /// JSONL streaming (one JSON object per line, flushed immediately)
    Stream,
}

pub struct SyncOptions {
    pub output: OutputMode,
    #[allow(dead_code)]
    pub mark_read: bool,
    pub download_media: bool,
    pub ignore_chat_ids: Vec<i64>,
    pub ignore_channels: bool,
    pub show_progress: bool,
    pub incremental: bool,
    pub messages_per_chat: usize,
    pub concurrency: usize,
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

/// Summary of messages synced for a forum topic
#[derive(Debug, Clone, serde::Serialize)]
pub struct TopicSyncSummary {
    pub topic_id: i32,
    pub topic_name: String,
    pub messages_synced: u64,
}

/// Summary of messages synced for a single chat
#[derive(Debug, Clone, serde::Serialize)]
pub struct ChatSyncSummary {
    pub chat_id: i64,
    pub chat_name: String,
    pub messages_synced: u64,
    /// For forum chats, breakdown by topic
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub topics: Vec<TopicSyncSummary>,
}

pub struct SyncResult {
    pub messages_stored: u64,
    pub chats_stored: u64,
    pub per_chat: Vec<ChatSyncSummary>,
}

/// Result from syncing a single chat (used for concurrent processing)
struct ChatSyncTaskResult {
    chat_id: i64,
    chat_name: String,
    chat_kind: String,
    chat_username: Option<String>,
    is_forum: bool,
    access_hash: Option<i64>,
    messages: Vec<FetchedMessage>,
    highest_msg_id: Option<i64>,
    latest_ts: Option<DateTime<Utc>>,
    topic_counts: std::collections::HashMap<i32, u64>,
    error: Option<String>,
}

/// A fetched message ready to be stored
struct FetchedMessage {
    id: i64,
    sender_id: i64,
    ts: DateTime<Utc>,
    edit_ts: Option<DateTime<Utc>>,
    from_me: bool,
    text: String,
    media_type: Option<String>,
    media_path: Option<String>,
    reply_to_id: Option<i64>,
    topic_id: Option<i32>,
}

impl App {
    /// Try to resolve a chat ID to a PeerRef from the session cache (no API calls).
    /// If the peer is not in the session cache but we have a stored access_hash, use that.
    /// Returns None if the chat is not cached and we have no stored access_hash.
    fn resolve_peer_from_session(
        &self,
        chat_id: i64,
        kind: &str,
        stored_access_hash: Option<i64>,
    ) -> Option<PeerRef> {
        // Try based on the known type first
        match kind {
            "channel" | "group" => {
                // Channels and megagroups use channel peer IDs
                let channel_peer_id = PeerId::channel(chat_id);
                if let Some(info) = self.tg.session.peer(channel_peer_id) {
                    return Some(PeerRef {
                        id: channel_peer_id,
                        auth: info.auth(),
                    });
                }
                // Fallback to stored access_hash if available
                if let Some(hash) = stored_access_hash {
                    return Some(PeerRef {
                        id: channel_peer_id,
                        auth: PeerAuth::from_hash(hash),
                    });
                }
            }
            "user" => {
                let user_peer_id = PeerId::user(chat_id);
                if let Some(info) = self.tg.session.peer(user_peer_id) {
                    return Some(PeerRef {
                        id: user_peer_id,
                        auth: info.auth(),
                    });
                }
                // Fallback to stored access_hash if available
                if let Some(hash) = stored_access_hash {
                    return Some(PeerRef {
                        id: user_peer_id,
                        auth: PeerAuth::from_hash(hash),
                    });
                }
            }
            _ => {}
        }

        // Fallback: try all peer types from session cache
        // Try as channel first (most common for groups)
        let channel_peer_id = PeerId::channel(chat_id);
        if let Some(info) = self.tg.session.peer(channel_peer_id) {
            return Some(PeerRef {
                id: channel_peer_id,
                auth: info.auth(),
            });
        }

        // Try as user
        let user_peer_id = PeerId::user(chat_id);
        if let Some(info) = self.tg.session.peer(user_peer_id) {
            return Some(PeerRef {
                id: user_peer_id,
                auth: info.auth(),
            });
        }

        // Try as small group chat (basic groups have different IDs)
        if chat_id > 0 && chat_id <= 999999999999 {
            let chat_peer_id = PeerId::chat(chat_id);
            if let Some(info) = self.tg.session.peer(chat_peer_id) {
                return Some(PeerRef {
                    id: chat_peer_id,
                    auth: info.auth(),
                });
            }
            // Small group chats don't need access_hash, so try with default auth
            if stored_access_hash.is_some() {
                return Some(PeerRef {
                    id: chat_peer_id,
                    auth: PeerAuth::default(),
                });
            }
        }

        // Last resort: try to construct from stored access_hash with best-guess peer type
        if let Some(hash) = stored_access_hash {
            // Most likely a channel/group if we have an access_hash
            return Some(PeerRef {
                id: channel_peer_id,
                auth: PeerAuth::from_hash(hash),
            });
        }

        None
    }

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

    /// Sync only chat list from Telegram dialogs (no messages).
    /// This fetches both active and archived dialogs and stores/updates chat metadata.
    pub async fn sync_chats(&mut self, opts: SyncOptions) -> Result<SyncResult> {
        let mut chats_stored: u64 = 0;

        // Build ignore set for fast lookup.
        let ignore_set: HashSet<i64> = opts.ignore_chat_ids.iter().copied().collect();

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

        // Phase 1: Fetch active dialogs
        if opts.show_progress {
            eprint!("\rSyncing chats... 0");
        }

        let mut dialogs = client.iter_dialogs();
        while let Some(dialog) = dialogs
            .next()
            .await
            .context("Failed to fetch dialogs from Telegram")?
        {
            let peer = dialog.peer();
            let (kind, name, username, is_forum, access_hash) = peer_info(peer);
            let id = peer.id().bare_id();

            if should_ignore(id, &kind) {
                continue;
            }

            self.store
                .upsert_chat(
                    id,
                    &kind,
                    &name,
                    username.as_deref(),
                    None,
                    is_forum,
                    access_hash,
                )
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

            // If it's a forum, sync topics
            if is_forum {
                if let Ok(topic_count) = self.sync_topics(id).await {
                    log::info!("Synced {} topics for forum chat {}", topic_count, id);
                }
            }

            if opts.show_progress && chats_stored.is_multiple_of(10) {
                eprint!("\rSyncing chats... {}", chats_stored);
            }
        }

        // Phase 2: Fetch archived dialogs
        if opts.show_progress {
            eprint!("\rSyncing archived chats... {}", chats_stored);
        }

        let archived_peers = self.fetch_archived_dialogs().await?;
        for peer in archived_peers {
            let (kind, name, username, is_forum, access_hash) = peer_info(&peer);
            let id = peer.id().bare_id();

            if should_ignore(id, &kind) {
                continue;
            }

            self.store
                .upsert_chat(
                    id,
                    &kind,
                    &name,
                    username.as_deref(),
                    None,
                    is_forum,
                    access_hash,
                )
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
            eprint!("\r\x1b[K"); // Clear line
        }
        eprintln!("Chats sync complete: {} chats", chats_stored);

        Ok(SyncResult {
            messages_stored: 0,
            chats_stored,
            per_chat: Vec::new(),
        })
    }

    /// Sync only messages from existing local chats (uses stored access_hash).
    /// This does NOT fetch dialogs from Telegram - it only syncs messages for chats
    /// that already exist in the local database with checkpoints.
    /// Uses concurrent fetching with semaphore-based rate limiting.
    pub async fn sync_msgs(&mut self, opts: SyncOptions) -> Result<SyncResult> {
        // Build ignore set for fast lookup.
        let ignore_set: HashSet<i64> = opts.ignore_chat_ids.iter().copied().collect();
        let ignore_channels = opts.ignore_channels;

        // Get all chats that have sync checkpoints
        let all_chats = self.store.list_chats_with_checkpoint().await?;

        // Filter chats to process
        let chats_to_sync: Vec<_> = all_chats
            .into_iter()
            .filter(|chat| {
                if ignore_set.contains(&chat.id) {
                    return false;
                }
                if ignore_channels && chat.kind == "channel" {
                    return false;
                }
                // Must have peer info to sync
                self.resolve_peer_from_session(chat.id, &chat.kind, chat.access_hash)
                    .is_some()
            })
            .collect();

        let total_chats = chats_to_sync.len();
        if total_chats == 0 {
            if opts.show_progress {
                eprintln!("Messages sync complete: 0 chats to sync");
            }
            return Ok(SyncResult {
                messages_stored: 0,
                chats_stored: 0,
                per_chat: Vec::new(),
            });
        }

        // Progress tracking with atomics for thread-safety
        let chats_done = Arc::new(AtomicU64::new(0));
        let messages_fetched = Arc::new(AtomicU64::new(0));

        // Semaphore for concurrency control
        let concurrency = opts.concurrency.max(1);
        let semaphore = Arc::new(Semaphore::new(concurrency));

        // Clone client for use in tasks (grammers Client is Clone)
        let client = self.tg.client.clone();

        // Session for peer resolution
        let session = self.tg.session.clone();

        // Store dir for media paths (if download enabled later)
        let store_dir = self.store_dir.clone();
        let download_media = opts.download_media;

        // Progress output task
        let show_progress = opts.show_progress;
        let chats_done_progress = chats_done.clone();
        let messages_fetched_progress = messages_fetched.clone();

        // Spawn progress reporter if needed
        let progress_handle = if show_progress {
            let total = total_chats;
            Some(tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_millis(500));
                loop {
                    interval.tick().await;
                    let done = chats_done_progress.load(Ordering::Relaxed);
                    let msgs = messages_fetched_progress.load(Ordering::Relaxed);
                    eprint!(
                        "\rSyncing messages... {}/{} chats, {} messages",
                        done, total, msgs
                    );
                    if done >= total as u64 {
                        break;
                    }
                }
            }))
        } else {
            None
        };

        // Create concurrent stream of chat sync tasks
        let results: Vec<ChatSyncTaskResult> = stream::iter(chats_to_sync)
            .map(|chat| {
                let sem = semaphore.clone();
                let client = client.clone();
                let session = session.clone();
                let store_dir = store_dir.clone();
                let chats_done = chats_done.clone();
                let messages_fetched = messages_fetched.clone();

                async move {
                    let _permit = sem.acquire().await.unwrap();

                    // Resolve peer
                    let peer_ref = resolve_peer_from_session_static(
                        &session,
                        chat.id,
                        &chat.kind,
                        chat.access_hash,
                    );

                    let peer_ref = match peer_ref {
                        Some(p) => p,
                        None => {
                            chats_done.fetch_add(1, Ordering::Relaxed);
                            return ChatSyncTaskResult {
                                chat_id: chat.id,
                                chat_name: chat.name.clone(),
                                chat_kind: chat.kind.clone(),
                                chat_username: chat.username.clone(),
                                is_forum: chat.is_forum,
                                access_hash: chat.access_hash,
                                messages: Vec::new(),
                                highest_msg_id: None,
                                latest_ts: None,
                                topic_counts: std::collections::HashMap::new(),
                                error: Some("No peer ref available".to_string()),
                            };
                        }
                    };

                    // Fetch messages
                    let last_sync_id = chat.last_sync_message_id;
                    let mut message_iter = client.iter_messages(peer_ref);
                    let mut messages = Vec::new();
                    let mut highest_msg_id: Option<i64> = None;
                    let mut latest_ts: Option<DateTime<Utc>> = None;
                    let mut topic_counts: std::collections::HashMap<i32, u64> =
                        std::collections::HashMap::new();
                    let mut error: Option<String> = None;

                    loop {
                        match message_iter.next().await {
                            Ok(Some(msg)) => {
                                let msg_id = msg.id() as i64;

                                // Stop when we hit a message we've already seen
                                if let Some(last_id) = last_sync_id {
                                    if msg_id <= last_id {
                                        break;
                                    }
                                }

                                if messages.len() >= INCREMENTAL_MAX_MESSAGES {
                                    break;
                                }

                                // Track highest message ID
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
                                let topic_id = if chat.is_forum {
                                    extract_topic_id(&msg)
                                } else {
                                    None
                                };

                                // Track per-topic counts
                                if let Some(tid) = topic_id {
                                    *topic_counts.entry(tid).or_insert(0) += 1;
                                }

                                // Handle media
                                let (media_type, media_path) = if download_media {
                                    download_message_media_static(
                                        &client, &msg, chat.id, &store_dir,
                                    )
                                    .await
                                    .unwrap_or((None, None))
                                } else {
                                    (msg.media().map(|_| "media".to_string()), None)
                                };

                                messages.push(FetchedMessage {
                                    id: msg_id,
                                    sender_id,
                                    ts: msg_ts,
                                    edit_ts: msg.edit_date(),
                                    from_me,
                                    text,
                                    media_type,
                                    media_path,
                                    reply_to_id,
                                    topic_id,
                                });

                                messages_fetched.fetch_add(1, Ordering::Relaxed);
                            }
                            Ok(None) => break,
                            Err(e) => {
                                error = Some(format!(
                                    "Failed to fetch messages for chat {} ({}): {}",
                                    chat.name, chat.id, e
                                ));
                                break;
                            }
                        }
                    }

                    chats_done.fetch_add(1, Ordering::Relaxed);

                    ChatSyncTaskResult {
                        chat_id: chat.id,
                        chat_name: chat.name.clone(),
                        chat_kind: chat.kind.clone(),
                        chat_username: chat.username.clone(),
                        is_forum: chat.is_forum,
                        access_hash: chat.access_hash,
                        messages,
                        highest_msg_id,
                        latest_ts,
                        topic_counts,
                        error,
                    }
                }
            })
            .buffer_unordered(concurrency)
            .collect()
            .await;

        // Stop progress reporter
        if let Some(handle) = progress_handle {
            handle.abort();
        }

        if show_progress {
            eprint!("\r\x1b[K"); // Clear line
        }

        // Now process results and write to store
        let mut messages_stored: u64 = 0;
        let mut chats_processed: u64 = 0;
        let mut per_chat_map: std::collections::HashMap<i64, ChatSyncSummary> =
            std::collections::HashMap::new();

        for result in results {
            // Log errors but continue
            if let Some(err) = &result.error {
                log::warn!("{}", err);
            }

            if result.messages.is_empty() && result.highest_msg_id.is_none() {
                continue;
            }

            chats_processed += 1;

            // Write messages to store
            for msg in &result.messages {
                // Output based on mode
                match opts.output {
                    OutputMode::Text => {
                        let from_label = if msg.from_me {
                            "me".to_string()
                        } else {
                            msg.sender_id.to_string()
                        };
                        let short_text = msg.text.replace('\n', " ");
                        let short_text = if short_text.len() > 100 {
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
                            from_label, result.chat_id, msg.id, short_text
                        );
                    }
                    OutputMode::Json => {
                        let obj = serde_json::json!({
                            "from_me": msg.from_me,
                            "sender": msg.sender_id,
                            "chat": result.chat_id,
                            "id": msg.id,
                            "timestamp": msg.ts.to_rfc3339(),
                            "text": msg.text,
                        });
                        println!("{}", serde_json::to_string(&obj).unwrap_or_default());
                    }
                    OutputMode::Stream => {
                        use std::io::Write;
                        let obj = serde_json::json!({
                            "type": "message",
                            "from_me": msg.from_me,
                            "sender_id": msg.sender_id,
                            "chat_id": result.chat_id,
                            "id": msg.id,
                            "ts": msg.ts.to_rfc3339(),
                            "text": msg.text,
                            "topic_id": msg.topic_id,
                            "media_type": msg.media_type,
                        });
                        println!("{}", serde_json::to_string(&obj).unwrap_or_default());
                        let _ = std::io::stdout().flush();
                    }
                    OutputMode::None => {}
                }

                self.store
                    .upsert_message(UpsertMessageParams {
                        id: msg.id,
                        chat_id: result.chat_id,
                        sender_id: msg.sender_id,
                        ts: msg.ts,
                        edit_ts: msg.edit_ts,
                        from_me: msg.from_me,
                        text: msg.text.clone(),
                        media_type: msg.media_type.clone(),
                        media_path: msg.media_path.clone(),
                        reply_to_id: msg.reply_to_id,
                        topic_id: msg.topic_id,
                    })
                    .await?;
                messages_stored += 1;
            }

            // Update chat's last_message_ts if we got new messages
            if let Some(ts) = result.latest_ts {
                self.store
                    .upsert_chat(
                        result.chat_id,
                        &result.chat_kind,
                        &result.chat_name,
                        result.chat_username.as_deref(),
                        Some(ts),
                        result.is_forum,
                        result.access_hash,
                    )
                    .await?;
            }

            // Update last_sync_message_id for incremental sync
            if let Some(high_id) = result.highest_msg_id {
                self.store
                    .update_last_sync_message_id(result.chat_id, high_id)
                    .await?;
            }

            // Track per-chat summary if messages were synced
            if !result.messages.is_empty() {
                // Build topic summaries for forums
                let new_topics: Vec<TopicSyncSummary> =
                    if result.is_forum && !result.topic_counts.is_empty() {
                        let mut topic_summaries = Vec::new();
                        for (tid, msg_count) in &result.topic_counts {
                            let topic_name = self
                                .store
                                .get_topic(result.chat_id, *tid)
                                .await
                                .ok()
                                .flatten()
                                .map(|t| t.name.clone())
                                .unwrap_or_else(|| format!("Topic {}", tid));
                            topic_summaries.push(TopicSyncSummary {
                                topic_id: *tid,
                                topic_name,
                                messages_synced: *msg_count,
                            });
                        }
                        topic_summaries
                    } else {
                        Vec::new()
                    };

                per_chat_map
                    .entry(result.chat_id)
                    .and_modify(|existing| {
                        existing.messages_synced += result.messages.len() as u64;
                        for new_topic in &new_topics {
                            if let Some(existing_topic) = existing
                                .topics
                                .iter_mut()
                                .find(|t| t.topic_id == new_topic.topic_id)
                            {
                                existing_topic.messages_synced += new_topic.messages_synced;
                            } else {
                                existing.topics.push(new_topic.clone());
                            }
                        }
                    })
                    .or_insert(ChatSyncSummary {
                        chat_id: result.chat_id,
                        chat_name: result.chat_name.clone(),
                        messages_synced: result.messages.len() as u64,
                        topics: new_topics,
                    });
            }
        }

        eprintln!(
            "Messages sync complete: {} chats checked, {} messages (concurrency: {})",
            chats_processed, messages_stored, concurrency
        );

        // Convert HashMap to Vec and sort topics by message count descending
        let per_chat: Vec<ChatSyncSummary> = per_chat_map
            .into_values()
            .map(|mut summary| {
                summary
                    .topics
                    .sort_by(|a, b| b.messages_synced.cmp(&a.messages_synced));
                summary
            })
            .collect();

        Ok(SyncResult {
            messages_stored,
            chats_stored: chats_processed,
            per_chat,
        })
    }

    /// Full sync: sync chats first, then sync messages.
    /// This is the default behavior when running `tgcli sync` without subcommands.
    pub async fn sync(&mut self, opts: SyncOptions) -> Result<SyncResult> {
        let mut messages_stored: u64 = 0;
        let mut chats_stored: u64 = 0;
        let mut per_chat_map: std::collections::HashMap<i64, ChatSyncSummary> =
            std::collections::HashMap::new();

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
            let (kind, name, username, is_forum, access_hash) = peer_info(peer);
            let id = peer.id().bare_id();

            // Skip ignored chats.
            if should_ignore(id, &kind) {
                continue;
            }

            self.store
                .upsert_chat(
                    id,
                    &kind,
                    &name,
                    username.as_deref(),
                    None,
                    is_forum,
                    access_hash,
                )
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
            // Track per-topic message counts for forums
            let mut topic_counts: std::collections::HashMap<i32, u64> =
                std::collections::HashMap::new();

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

                // Track per-topic counts for forums
                if let Some(tid) = topic_id {
                    *topic_counts.entry(tid).or_insert(0) += 1;
                }

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
                    .upsert_chat(
                        id,
                        &kind,
                        &name,
                        username.as_deref(),
                        Some(ts),
                        is_forum,
                        access_hash,
                    )
                    .await?;
            }

            // Update last_sync_message_id for incremental sync
            if let Some(high_id) = highest_msg_id {
                self.store.update_last_sync_message_id(id, high_id).await?;
            }

            // If it's a forum, sync topics first so we can get names
            if is_forum {
                if let Ok(topic_count) = self.sync_topics(id).await {
                    log::info!("Synced {} topics for forum chat {}", topic_count, id);
                }
            }

            // Track per-chat summary if messages were synced
            if count > 0 {
                // Build topic summaries for forums
                let new_topics: Vec<TopicSyncSummary> = if is_forum && !topic_counts.is_empty() {
                    let mut topic_summaries = Vec::new();
                    for (tid, msg_count) in &topic_counts {
                        let topic_name = self
                            .store
                            .get_topic(id, *tid)
                            .await
                            .ok()
                            .flatten()
                            .map(|t| t.name.clone())
                            .unwrap_or_else(|| format!("Topic {}", tid));
                        topic_summaries.push(TopicSyncSummary {
                            topic_id: *tid,
                            topic_name,
                            messages_synced: *msg_count,
                        });
                    }
                    topic_summaries
                } else {
                    Vec::new()
                };

                // Aggregate into per_chat_map
                per_chat_map
                    .entry(id)
                    .and_modify(|existing| {
                        existing.messages_synced += count as u64;
                        // Merge topics by topic_id
                        for new_topic in &new_topics {
                            if let Some(existing_topic) = existing
                                .topics
                                .iter_mut()
                                .find(|t| t.topic_id == new_topic.topic_id)
                            {
                                existing_topic.messages_synced += new_topic.messages_synced;
                            } else {
                                existing.topics.push(new_topic.clone());
                            }
                        }
                    })
                    .or_insert(ChatSyncSummary {
                        chat_id: id,
                        chat_name: name.clone(),
                        messages_synced: count as u64,
                        topics: new_topics,
                    });
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
            let (kind, name, username, is_forum, access_hash) = peer_info(&peer);
            let id = peer.id().bare_id();

            // Skip ignored chats.
            if should_ignore(id, &kind) {
                continue;
            }

            self.store
                .upsert_chat(
                    id,
                    &kind,
                    &name,
                    username.as_deref(),
                    None,
                    is_forum,
                    access_hash,
                )
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
            // Track per-topic message counts for forums
            let mut topic_counts: std::collections::HashMap<i32, u64> =
                std::collections::HashMap::new();

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

                // Track per-topic counts for forums
                if let Some(tid) = topic_id {
                    *topic_counts.entry(tid).or_insert(0) += 1;
                }

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
                    .upsert_chat(
                        id,
                        &kind,
                        &name,
                        username.as_deref(),
                        Some(ts),
                        is_forum,
                        access_hash,
                    )
                    .await?;
            }

            // Update last_sync_message_id for incremental sync
            if let Some(high_id) = highest_msg_id {
                self.store.update_last_sync_message_id(id, high_id).await?;
            }

            // If it's a forum, sync topics first so we can get names
            if is_forum {
                if let Ok(topic_count) = self.sync_topics(id).await {
                    log::info!(
                        "Synced {} topics for archived forum chat {}",
                        topic_count,
                        id
                    );
                }
            }

            // Track per-chat summary if messages were synced
            if count > 0 {
                // Build topic summaries for forums
                let new_topics: Vec<TopicSyncSummary> = if is_forum && !topic_counts.is_empty() {
                    let mut topic_summaries = Vec::new();
                    for (tid, msg_count) in &topic_counts {
                        let topic_name = self
                            .store
                            .get_topic(id, *tid)
                            .await
                            .ok()
                            .flatten()
                            .map(|t| t.name.clone())
                            .unwrap_or_else(|| format!("Topic {}", tid));
                        topic_summaries.push(TopicSyncSummary {
                            topic_id: *tid,
                            topic_name,
                            messages_synced: *msg_count,
                        });
                    }
                    topic_summaries
                } else {
                    Vec::new()
                };

                // Aggregate into per_chat_map
                per_chat_map
                    .entry(id)
                    .and_modify(|existing| {
                        existing.messages_synced += count as u64;
                        // Merge topics by topic_id
                        for new_topic in &new_topics {
                            if let Some(existing_topic) = existing
                                .topics
                                .iter_mut()
                                .find(|t| t.topic_id == new_topic.topic_id)
                            {
                                existing_topic.messages_synced += new_topic.messages_synced;
                            } else {
                                existing.topics.push(new_topic.clone());
                            }
                        }
                    })
                    .or_insert(ChatSyncSummary {
                        chat_id: id,
                        chat_name: name.clone(),
                        messages_synced: count as u64,
                        topics: new_topics,
                    });
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

        // Convert HashMap to Vec and sort topics by message count descending
        let per_chat: Vec<ChatSyncSummary> = per_chat_map
            .into_values()
            .map(|mut summary| {
                summary
                    .topics
                    .sort_by(|a, b| b.messages_synced.cmp(&a.messages_synced));
                summary
            })
            .collect();

        Ok(SyncResult {
            messages_stored,
            chats_stored,
            per_chat,
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

/// Static version of resolve_peer_from_session for use in async tasks
fn resolve_peer_from_session_static(
    session: &SqliteSession,
    chat_id: i64,
    kind: &str,
    stored_access_hash: Option<i64>,
) -> Option<PeerRef> {
    // Try based on the known type first
    match kind {
        "channel" | "group" => {
            let channel_peer_id = PeerId::channel(chat_id);
            if let Some(info) = session.peer(channel_peer_id) {
                return Some(PeerRef {
                    id: channel_peer_id,
                    auth: info.auth(),
                });
            }
            if let Some(hash) = stored_access_hash {
                return Some(PeerRef {
                    id: channel_peer_id,
                    auth: PeerAuth::from_hash(hash),
                });
            }
        }
        "user" => {
            let user_peer_id = PeerId::user(chat_id);
            if let Some(info) = session.peer(user_peer_id) {
                return Some(PeerRef {
                    id: user_peer_id,
                    auth: info.auth(),
                });
            }
            if let Some(hash) = stored_access_hash {
                return Some(PeerRef {
                    id: user_peer_id,
                    auth: PeerAuth::from_hash(hash),
                });
            }
        }
        _ => {}
    }

    // Fallback: try all peer types from session cache
    let channel_peer_id = PeerId::channel(chat_id);
    if let Some(info) = session.peer(channel_peer_id) {
        return Some(PeerRef {
            id: channel_peer_id,
            auth: info.auth(),
        });
    }

    let user_peer_id = PeerId::user(chat_id);
    if let Some(info) = session.peer(user_peer_id) {
        return Some(PeerRef {
            id: user_peer_id,
            auth: info.auth(),
        });
    }

    if chat_id > 0 && chat_id <= 999999999999 {
        let chat_peer_id = PeerId::chat(chat_id);
        if let Some(info) = session.peer(chat_peer_id) {
            return Some(PeerRef {
                id: chat_peer_id,
                auth: info.auth(),
            });
        }
        if stored_access_hash.is_some() {
            return Some(PeerRef {
                id: chat_peer_id,
                auth: PeerAuth::default(),
            });
        }
    }

    if let Some(hash) = stored_access_hash {
        return Some(PeerRef {
            id: channel_peer_id,
            auth: PeerAuth::from_hash(hash),
        });
    }

    None
}

/// Static version of download_message_media for use in async tasks
async fn download_message_media_static(
    client: &Client,
    msg: &TgMessage,
    chat_id: i64,
    store_dir: &str,
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
    let media_dir = Path::new(store_dir).join("media").join(chat_id.to_string());

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
    match client.download_media(&media, &file_path).await {
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
            Ok((Some(media_type), None))
        }
    }
}

/// Returns (kind, name, username, is_forum, access_hash)
fn peer_info(peer: &Peer) -> (String, String, Option<String>, bool, Option<i64>) {
    match peer {
        Peer::User(user) => {
            let name = user.full_name();
            let username = user.username().map(|s| s.to_string());
            // Extract access_hash from User raw type
            let access_hash = match &user.raw {
                tl::enums::User::User(u) => u.access_hash,
                tl::enums::User::Empty(_) => None,
            };
            ("user".to_string(), name, username, false, access_hash)
        }
        Peer::Group(group) => {
            let name = group.title().map(|s| s.to_string()).unwrap_or_default();
            let username = group.username().map(|s| s.to_string());
            // Check if this is a forum group (megagroup with forum flag)
            // Extract access_hash from Channel (megagroups are channels internally)
            let (is_forum, access_hash) = match &group.raw {
                tl::enums::Chat::Channel(channel) => (channel.forum, channel.access_hash),
                _ => (false, None),
            };
            ("group".to_string(), name, username, is_forum, access_hash)
        }
        Peer::Channel(channel) => {
            let name = channel.title().to_string();
            let username = channel.username().map(|s| s.to_string());
            // Extract access_hash directly from Channel raw type
            let access_hash = channel.raw.access_hash;
            ("channel".to_string(), name, username, false, access_hash)
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
