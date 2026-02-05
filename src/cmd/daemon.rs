//! Daemon mode for persistent real-time message sync.
//!
//! This command starts a persistent Telegram client that:
//! 1. Immediately subscribes to real-time updates
//! 2. Saves incoming messages to the local database as they arrive
//! 3. Optionally runs background incremental sync to catch up on missed messages

use crate::app::App;
use crate::store::UpsertMessageParams;
use crate::Cli;
use anyhow::{Context, Result};
use chrono::Utc;
use clap::Args;
use grammers_client::types::Peer;
use grammers_client::{Update, UpdatesConfiguration};
use grammers_tl_types as tl;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Args, Debug, Clone)]
pub struct DaemonArgs {
    /// Don't run background sync (only listen for new updates)
    #[arg(long, default_value_t = false)]
    pub no_backfill: bool,

    /// Download media files for incoming messages
    #[arg(long, default_value_t = false)]
    pub download_media: bool,

    /// Chat IDs to ignore (skip during sync and updates)
    #[arg(long = "ignore", value_name = "CHAT_ID")]
    pub ignore_chat_ids: Vec<i64>,

    /// Skip all channel updates
    #[arg(long, default_value_t = false)]
    pub ignore_channels: bool,

    /// Suppress progress output
    #[arg(long, default_value_t = false)]
    pub quiet: bool,

    /// Output updates as JSONL stream to stdout
    #[arg(long, default_value_t = false)]
    pub stream: bool,
}

/// Extract chat_id from a Peer
fn extract_chat_id_from_peer(peer: &Peer) -> i64 {
    peer.id().bare_id()
}

/// Extract sender_id from a Message update
fn extract_sender_id(msg: &grammers_client::types::update::Message) -> i64 {
    msg.sender().map(|s| s.id().bare_id()).unwrap_or(0)
}

/// Extract topic_id from a raw update if present
fn extract_topic_id_from_raw(raw: &tl::enums::Update) -> Option<i32> {
    match raw {
        tl::enums::Update::NewChannelMessage(m) => extract_topic_from_message(&m.message),
        tl::enums::Update::EditChannelMessage(m) => extract_topic_from_message(&m.message),
        _ => None,
    }
}

fn extract_topic_from_message(msg: &tl::enums::Message) -> Option<i32> {
    if let tl::enums::Message::Message(m) = msg {
        if let Some(tl::enums::MessageReplyHeader::Header(header)) = &m.reply_to {
            if header.forum_topic {
                return header.reply_to_top_id.or(header.reply_to_msg_id);
            }
        }
    }
    None
}

/// Determine chat kind from peer
fn chat_kind_from_peer(peer: &Peer) -> &'static str {
    match peer {
        Peer::User(_) => "user",
        Peer::Group(_) => "group",
        Peer::Channel(c) => {
            // Channel.raw.megagroup indicates if this is actually a megagroup (supergroup)
            if c.raw.megagroup {
                "group"
            } else {
                "channel"
            }
        }
    }
}

/// Get chat name from Peer
fn chat_name_from_peer(peer: &Peer) -> String {
    match peer {
        Peer::User(u) => u
            .first_name()
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("User {}", u.bare_id())),
        Peer::Group(g) => g
            .title()
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("Group {}", g.id().bare_id())),
        Peer::Channel(c) => c.title().to_string(),
    }
}

/// Get username from Peer if available
fn username_from_peer(peer: &Peer) -> Option<String> {
    match peer {
        Peer::User(u) => u.username().map(|s| s.to_string()),
        Peer::Channel(c) => c.username().map(|s| s.to_string()),
        Peer::Group(_) => None,
    }
}

/// Check if Peer is a forum
fn is_forum_peer(peer: &Peer) -> bool {
    matches!(peer, Peer::Channel(c) if c.raw.forum)
}

/// Get access_hash from Peer
fn access_hash_from_peer(peer: &Peer) -> Option<i64> {
    match peer {
        Peer::User(u) => {
            // User.raw is tl::enums::User, need to match to get inner tl::types::User
            match &u.raw {
                tl::enums::User::User(user) => user.access_hash,
                tl::enums::User::Empty(_) => None,
            }
        }
        Peer::Channel(c) => c.raw.access_hash,
        Peer::Group(_) => None, // Basic groups don't have access_hash
    }
}

pub async fn run(cli: &Cli, args: &DaemonArgs) -> Result<()> {
    let mut app = App::new(cli).await?;

    // Take ownership of the updates receiver
    let updates_rx = app
        .updates_rx
        .take()
        .context("Updates receiver not available")?;

    let ignore_set: HashSet<i64> = args.ignore_chat_ids.iter().copied().collect();
    let ignore_channels = args.ignore_channels;

    // Counters for statistics
    let messages_received = Arc::new(AtomicU64::new(0));
    let messages_stored = Arc::new(AtomicU64::new(0));
    let backfill_running = Arc::new(AtomicBool::new(false));
    let shutdown = Arc::new(AtomicBool::new(false));

    if !args.quiet {
        eprintln!("Daemon starting...");
        eprintln!("  Listening for real-time updates");
        if !args.no_backfill {
            eprintln!("  Background sync will start after connection established");
        }
    }

    // Start the update stream - this subscribes to updates immediately
    // catch_up: true means it will also fetch any missed updates since last session
    let mut update_stream = app.tg.client.stream_updates(
        updates_rx,
        UpdatesConfiguration {
            catch_up: !args.no_backfill, // Catch up on missed updates if backfill enabled
            ..Default::default()
        },
    );

    // Spawn background sync task if backfill is enabled
    let backfill_handle = if !args.no_backfill {
        let cli_clone = cli.clone();
        let ignore_ids = args.ignore_chat_ids.clone();
        let ignore_chans = args.ignore_channels;
        let backfill_running_clone = Arc::clone(&backfill_running);
        let shutdown_clone = Arc::clone(&shutdown);
        let quiet = args.quiet;

        Some(tokio::spawn(async move {
            // Small delay to ensure update listener is fully established
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;

            if shutdown_clone.load(Ordering::Relaxed) {
                return Ok::<_, anyhow::Error>(());
            }

            backfill_running_clone.store(true, Ordering::Relaxed);
            if !quiet {
                eprintln!("Background sync starting...");
            }

            // Create a separate App instance for backfill
            let mut backfill_app = App::new(&cli_clone).await?;

            let opts = crate::app::sync::SyncOptions {
                output: crate::app::sync::OutputMode::None,
                mark_read: false,
                download_media: false,
                ignore_chat_ids: ignore_ids,
                ignore_channels: ignore_chans,
                show_progress: !quiet,
                incremental: true,
                messages_per_chat: 50,
                concurrency: 4,
            };

            let result = backfill_app.sync(opts).await;
            backfill_running_clone.store(false, Ordering::Relaxed);

            match result {
                Ok(res) => {
                    if !quiet {
                        eprintln!(
                            "Background sync complete: {} chats, {} messages",
                            res.chats_stored, res.messages_stored
                        );
                    }
                }
                Err(e) => {
                    log::error!("Background sync failed: {}", e);
                    if !quiet {
                        eprintln!("Background sync failed: {}", e);
                    }
                }
            }

            Ok(())
        }))
    } else {
        None
    };

    if !args.quiet {
        eprintln!("Daemon ready. Press Ctrl+C to stop.");
    }

    // Main update loop
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                if !args.quiet {
                    eprintln!("\nShutting down...");
                }
                shutdown.store(true, Ordering::Relaxed);
                break;
            }
            update_result = update_stream.next() => {
                match update_result {
                    Ok(update) => {
                        messages_received.fetch_add(1, Ordering::Relaxed);

                        match update {
                            Update::NewMessage(msg) => {
                                // Get the peer (chat) from the message
                                let peer = match msg.peer() {
                                    Ok(p) => p.clone(),
                                    Err(_) => {
                                        log::warn!("Could not resolve peer for message {}", msg.id());
                                        continue;
                                    }
                                };

                                let chat_id = extract_chat_id_from_peer(&peer);
                                let chat_kind = chat_kind_from_peer(&peer);

                                // Check ignore filters
                                if ignore_set.contains(&chat_id) {
                                    continue;
                                }
                                if ignore_channels && chat_kind == "channel" {
                                    continue;
                                }

                                let sender_id = extract_sender_id(&msg);
                                let from_me = msg.outgoing();
                                let text = msg.text().to_string();
                                let ts = msg.date();
                                let reply_to_id = msg.reply_to_message_id().map(|id| id as i64);
                                let topic_id = extract_topic_id_from_raw(&msg.raw);
                                let media_type = msg.media().map(|_| "media".to_string());

                                // Stream output if enabled
                                if args.stream {
                                    use std::io::Write;
                                    let obj = serde_json::json!({
                                        "type": "new_message",
                                        "chat_id": chat_id,
                                        "id": msg.id(),
                                        "sender_id": sender_id,
                                        "from_me": from_me,
                                        "ts": ts.to_rfc3339(),
                                        "text": text,
                                        "topic_id": topic_id,
                                        "media_type": media_type,
                                    });
                                    println!("{}", serde_json::to_string(&obj).unwrap_or_default());
                                    let _ = std::io::stdout().flush();
                                }

                                // Store message directly
                                if let Err(e) = app.store.upsert_message(UpsertMessageParams {
                                    id: msg.id() as i64,
                                    chat_id,
                                    sender_id,
                                    ts,
                                    edit_ts: None,
                                    from_me,
                                    text,
                                    media_type,
                                    media_path: None, // TODO: download media if enabled
                                    reply_to_id,
                                    topic_id,
                                }).await {
                                    log::error!("Failed to store message: {}", e);
                                } else {
                                    messages_stored.fetch_add(1, Ordering::Relaxed);
                                }

                                // Update chat metadata
                                let chat_name = chat_name_from_peer(&peer);
                                let username = username_from_peer(&peer);
                                let is_forum = is_forum_peer(&peer);
                                let access_hash = access_hash_from_peer(&peer);

                                if let Err(e) = app.store.upsert_chat(
                                    chat_id,
                                    chat_kind,
                                    &chat_name,
                                    username.as_deref(),
                                    Some(ts),
                                    is_forum,
                                    access_hash,
                                ).await {
                                    log::error!("Failed to update chat metadata: {}", e);
                                }

                                // Update last sync message ID
                                if let Err(e) = app.store.update_last_sync_message_id(chat_id, msg.id() as i64).await {
                                    log::error!("Failed to update last_sync_message_id: {}", e);
                                }
                            }
                            Update::MessageEdited(msg) => {
                                // Get the peer (chat) from the message
                                let peer = match msg.peer() {
                                    Ok(p) => p,
                                    Err(_) => {
                                        log::warn!("Could not resolve peer for edited message {}", msg.id());
                                        continue;
                                    }
                                };

                                let chat_id = extract_chat_id_from_peer(peer);

                                // Check ignore filters
                                if ignore_set.contains(&chat_id) {
                                    continue;
                                }

                                let text = msg.text().to_string();

                                // Stream output if enabled
                                if args.stream {
                                    use std::io::Write;
                                    let obj = serde_json::json!({
                                        "type": "message_edited",
                                        "chat_id": chat_id,
                                        "id": msg.id(),
                                        "text": text,
                                        "edit_ts": Utc::now().to_rfc3339(),
                                    });
                                    println!("{}", serde_json::to_string(&obj).unwrap_or_default());
                                    let _ = std::io::stdout().flush();
                                }

                                // Update message text
                                if let Err(e) = app.store.update_message_text(chat_id, msg.id() as i64, &text).await {
                                    log::error!("Failed to update edited message: {}", e);
                                }
                            }
                            Update::MessageDeleted(deletion) => {
                                // Extract deleted message IDs from raw update
                                let (chat_id, msg_ids) = match &deletion.raw {
                                    tl::enums::Update::DeleteMessages(d) => {
                                        (None, d.messages.clone())
                                    }
                                    tl::enums::Update::DeleteChannelMessages(d) => {
                                        (Some(d.channel_id), d.messages.clone())
                                    }
                                    _ => continue,
                                };

                                if args.stream {
                                    use std::io::Write;
                                    let obj = serde_json::json!({
                                        "type": "message_deleted",
                                        "chat_id": chat_id,
                                        "message_ids": msg_ids,
                                    });
                                    println!("{}", serde_json::to_string(&obj).unwrap_or_default());
                                    let _ = std::io::stdout().flush();
                                }

                                // Note: We don't delete from local DB by default
                                // Messages remain for history. Add --delete-on-remote-delete flag if needed.
                            }
                            Update::Raw(raw) => {
                                // Log unhandled update types for debugging
                                log::debug!("Unhandled raw update: {:?}", raw.raw);
                            }
                            _ => {
                                // CallbackQuery, InlineQuery, etc. - not relevant for message sync
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Update stream error: {}", e);
                        if !args.quiet {
                            eprintln!("Update stream error: {}", e);
                        }
                        // For transient errors, continue. For fatal errors, break.
                        if e.to_string().contains("Dropped") {
                            break;
                        }
                    }
                }
            }
        }
    }

    // Wait for backfill to finish if running
    if let Some(handle) = backfill_handle {
        if backfill_running.load(Ordering::Relaxed) && !args.quiet {
            eprintln!("Waiting for background sync to complete...");
        }
        let _ = handle.await;
    }

    // Sync update state to session before exit
    update_stream.sync_update_state();

    if !args.quiet {
        eprintln!(
            "Daemon stopped. Updates received: {}, stored: {}",
            messages_received.load(Ordering::Relaxed),
            messages_stored.load(Ordering::Relaxed)
        );
    }

    Ok(())
}
