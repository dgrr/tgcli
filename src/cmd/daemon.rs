//! Daemon mode for persistent real-time message sync with optional RPC server.
//!
//! This command starts a persistent Telegram client that:
//! 1. Immediately subscribes to real-time updates
//! 2. Saves incoming messages to the local database as they arrive
//! 3. Optionally runs background incremental sync to catch up on missed messages
//! 4. Optionally starts an HTTP RPC server for external access
//!
//! ## Production Features
//!
//! - Proper signal handling (SIGTERM, SIGINT, SIGHUP)
//! - Graceful shutdown with configurable timeout
//! - Health check endpoint (/ping, /status)
//! - Configurable bind address/port
//! - Structured logging with tracing
//! - PID file support for process management
//! - Systemd/launchd compatibility (foreground mode, proper exit codes)

use crate::app::App;
use crate::rpc::{RpcServer, RpcServerConfig, RpcState};
use crate::shutdown;
use crate::store::UpsertMessageParams;
use crate::Cli;
use anyhow::{Context, Result};
use chrono::Utc;
use clap::Args;
use grammers_client::types::Peer;
use grammers_client::{Update, UpdatesConfiguration};
use grammers_tl_types as tl;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Default RPC server port
const DEFAULT_RPC_ADDR: &str = "127.0.0.1:5556";

/// Default shutdown timeout in seconds
const DEFAULT_SHUTDOWN_TIMEOUT_SECS: u64 = 10;

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

    // === RPC Server Options ===
    /// Enable HTTP RPC server
    #[arg(long, default_value_t = false)]
    pub rpc: bool,

    /// RPC server listen address (e.g., "127.0.0.1:5556")
    #[arg(long, default_value = DEFAULT_RPC_ADDR)]
    pub rpc_addr: String,

    /// Write PID to this file (useful for process managers)
    #[arg(long)]
    pub pid_file: Option<PathBuf>,

    /// Shutdown timeout in seconds (for graceful shutdown)
    #[arg(long, default_value_t = DEFAULT_SHUTDOWN_TIMEOUT_SECS)]
    pub shutdown_timeout: u64,
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
        Peer::User(u) => match &u.raw {
            tl::enums::User::User(user) => user.access_hash,
            tl::enums::User::Empty(_) => None,
        },
        Peer::Channel(c) => c.raw.access_hash,
        Peer::Group(_) => None,
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

    // Get global shutdown controller
    let shutdown_ctrl = shutdown::global();

    // Set up SIGTERM handler (in addition to SIGINT which is already handled)
    #[cfg(unix)]
    {
        let shutdown_clone = shutdown_ctrl.clone();
        tokio::spawn(async move {
            let mut sigterm =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("Failed to register SIGTERM handler");
            sigterm.recv().await;
            log::info!("Received SIGTERM, initiating graceful shutdown...");
            shutdown_clone.trigger();
        });

        // Also handle SIGHUP for reload (could be used for config reload in the future)
        let shutdown_clone2 = shutdown_ctrl.clone();
        tokio::spawn(async move {
            let mut sighup =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())
                    .expect("Failed to register SIGHUP handler");
            loop {
                sighup.recv().await;
                log::info!("Received SIGHUP");
                // For now, SIGHUP triggers shutdown. Could be used for config reload later.
                shutdown_clone2.trigger();
                break;
            }
        });
    }

    // Counters for statistics
    let messages_received = Arc::new(AtomicU64::new(0));
    let messages_stored = Arc::new(AtomicU64::new(0));
    let backfill_running = Arc::new(AtomicBool::new(false));

    // Start RPC server if enabled
    let rpc_state: Option<Arc<RpcState>> = if args.rpc {
        let config = RpcServerConfig {
            addr: args.rpc_addr.clone(),
            store_dir: cli.store_dir(),
            pid_file: args.pid_file.clone(),
            request_timeout: Duration::from_secs(30),
        };

        let server = RpcServer::new(config).await?;
        let state = server.state();

        // Mark TG as connected
        state.set_tg_connected(true);

        // Spawn RPC server task
        let shutdown_clone = shutdown_ctrl.clone();
        tokio::spawn(async move {
            if let Err(e) = server.start(shutdown_clone).await {
                log::error!("RPC server error: {}", e);
            }
        });

        if !args.quiet {
            eprintln!("RPC server listening on http://{}", args.rpc_addr);
        }

        Some(state)
    } else {
        // Write PID file even without RPC if requested
        if let Some(ref pid_path) = args.pid_file {
            let pid = std::process::id();
            std::fs::write(pid_path, format!("{}\n", pid))
                .with_context(|| format!("Failed to write PID file: {:?}", pid_path))?;
            log::info!("PID {} written to {:?}", pid, pid_path);
        }
        None
    };

    if !args.quiet {
        eprintln!("Daemon starting...");
        eprintln!("  Listening for real-time updates");
        if !args.no_backfill {
            eprintln!("  Background sync will start after connection established");
        }
    }

    // Start the update stream
    let mut update_stream = app.tg.client.stream_updates(
        updates_rx,
        UpdatesConfiguration {
            catch_up: !args.no_backfill,
            ..Default::default()
        },
    );

    // Spawn background sync task if backfill is enabled
    let backfill_handle = if !args.no_backfill {
        let cli_clone = cli.clone();
        let ignore_ids = args.ignore_chat_ids.clone();
        let ignore_chans = args.ignore_channels;
        let backfill_running_clone = Arc::clone(&backfill_running);
        let shutdown_ctrl_clone = shutdown_ctrl.clone();
        let quiet = args.quiet;
        let rpc_state_clone = rpc_state.clone();

        Some(tokio::spawn(async move {
            // Small delay to ensure update listener is fully established
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(2)) => {}
                _ = shutdown_ctrl_clone.cancelled() => {
                    return Ok::<_, anyhow::Error>(());
                }
            }

            if shutdown_ctrl_clone.is_triggered() {
                return Ok::<_, anyhow::Error>(());
            }

            backfill_running_clone.store(true, Ordering::Relaxed);
            if let Some(ref state) = rpc_state_clone {
                state.set_sync_running(true);
            }

            if !quiet {
                eprintln!("Background sync starting...");
            }
            log::info!("Background sync starting");

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
                chat_filter: None,
                prune_after: None,
                skip_archived: false,
                archived_only: false,
            };

            let result = backfill_app.sync(opts).await;
            backfill_running_clone.store(false, Ordering::Relaxed);
            if let Some(ref state) = rpc_state_clone {
                state.set_sync_running(false);
            }

            match result {
                Ok(res) => {
                    log::info!(
                        "Background sync complete: {} chats, {} messages",
                        res.chats_stored, res.messages_stored
                    );
                    if !quiet {
                        eprintln!(
                            "Background sync complete: {} chats, {} messages",
                            res.chats_stored, res.messages_stored
                        );
                    }
                }
                Err(e) => {
                    if !shutdown_ctrl_clone.is_triggered() {
                        log::error!("Background sync failed: {}", e);
                        if !quiet {
                            eprintln!("Background sync failed: {}", e);
                        }
                    }
                }
            }

            Ok(())
        }))
    } else {
        None
    };

    // Load webhook config for firing on incoming messages
    let webhook_config = app.store.get_webhook().await.ok().flatten();
    if let Some(ref wh) = webhook_config {
        if !args.quiet {
            eprintln!("  Webhook active: {}", wh.url);
        }
        log::info!("Webhook active: {}", wh.url);
    }

    if !args.quiet {
        eprintln!("Daemon ready. Press Ctrl+C to stop.");
    }
    log::info!("Daemon ready");

    // Main update loop
    loop {
        tokio::select! {
            _ = shutdown_ctrl.cancelled() => {
                log::info!("Shutdown signal received");
                if !args.quiet {
                    eprintln!("\nShutting down gracefully...");
                }
                break;
            }
            update_result = update_stream.next() => {
                match update_result {
                    Ok(update) => {
                        messages_received.fetch_add(1, Ordering::Relaxed);
                        if let Some(ref state) = rpc_state {
                            state.increment_received();
                        }

                        match update {
                            Update::NewMessage(msg) => {
                                let peer = match msg.peer() {
                                    Ok(p) => p.clone(),
                                    Err(_) => {
                                        log::warn!("Could not resolve peer for message {}", msg.id());
                                        continue;
                                    }
                                };

                                let chat_id = extract_chat_id_from_peer(&peer);
                                let chat_kind = chat_kind_from_peer(&peer);

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

                                // Fire webhook for incoming messages
                                if !from_me {
                                    if let Some(ref wh) = webhook_config {
                                        crate::webhook::fire_webhook(wh, chat_id, sender_id, &text);
                                    }
                                }

                                // Store message
                                if let Err(e) = app.store.upsert_message(UpsertMessageParams {
                                    id: msg.id() as i64,
                                    chat_id,
                                    sender_id,
                                    ts,
                                    edit_ts: None,
                                    from_me,
                                    text,
                                    media_type,
                                    media_path: None,
                                    reply_to_id,
                                    topic_id,
                                }).await {
                                    log::error!("Failed to store message: {}", e);
                                } else {
                                    messages_stored.fetch_add(1, Ordering::Relaxed);
                                    if let Some(ref state) = rpc_state {
                                        state.increment_stored();
                                    }
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

                                if let Err(e) = app.store.update_last_sync_message_id(chat_id, msg.id() as i64).await {
                                    log::error!("Failed to update last_sync_message_id: {}", e);
                                }

                                log::debug!(
                                    "Message stored: chat_id={}, msg_id={}, from_me={}",
                                    chat_id, msg.id(), from_me
                                );
                            }
                            Update::MessageEdited(msg) => {
                                let peer = match msg.peer() {
                                    Ok(p) => p,
                                    Err(_) => {
                                        log::warn!("Could not resolve peer for edited message {}", msg.id());
                                        continue;
                                    }
                                };

                                let chat_id = extract_chat_id_from_peer(peer);

                                if ignore_set.contains(&chat_id) {
                                    continue;
                                }

                                let text = msg.text().to_string();

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

                                if let Err(e) = app.store.update_message_text(chat_id, msg.id() as i64, &text).await {
                                    log::error!("Failed to update edited message: {}", e);
                                }

                                log::debug!("Message edited: chat_id={}, msg_id={}", chat_id, msg.id());
                            }
                            Update::MessageDeleted(deletion) => {
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

                                log::debug!("Messages deleted: chat_id={:?}, count={}", chat_id, msg_ids.len());
                            }
                            Update::Raw(raw) => {
                                log::debug!("Unhandled raw update: {:?}", raw.raw);
                            }
                            _ => {
                                // CallbackQuery, InlineQuery, etc.
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Update stream error: {}", e);
                        if !args.quiet {
                            eprintln!("Update stream error: {}", e);
                        }
                        if e.to_string().contains("Dropped") {
                            break;
                        }
                    }
                }
            }
        }
    }

    // Graceful shutdown
    let shutdown_timeout = Duration::from_secs(args.shutdown_timeout);

    // Wait for backfill to finish
    if let Some(handle) = backfill_handle {
        if backfill_running.load(Ordering::Relaxed) {
            if !args.quiet {
                eprintln!("Waiting for background sync to complete (timeout: {}s)...", args.shutdown_timeout);
            }
            log::info!("Waiting for background sync (timeout: {}s)", args.shutdown_timeout);
        }
        let _ = tokio::time::timeout(shutdown_timeout, handle).await;
    }

    // Mark TG as disconnected for RPC
    if let Some(ref state) = rpc_state {
        state.set_tg_connected(false);
    }

    // Sync update state to session before exit
    if !args.quiet {
        eprintln!("Syncing session state...");
    }
    log::info!("Syncing session state");
    update_stream.sync_update_state();

    // Cleanup PID file if we wrote it (and RPC didn't already clean it up)
    if args.pid_file.is_some() && rpc_state.is_none() {
        if let Some(ref pid_path) = args.pid_file {
            let _ = std::fs::remove_file(pid_path);
        }
    }

    let final_received = messages_received.load(Ordering::Relaxed);
    let final_stored = messages_stored.load(Ordering::Relaxed);

    log::info!(
        "Daemon stopped. Updates received: {}, stored: {}",
        final_received, final_stored
    );

    if !args.quiet {
        eprintln!(
            "Daemon stopped. Updates received: {}, stored: {}",
            final_received, final_stored
        );
    }

    Ok(())
}
