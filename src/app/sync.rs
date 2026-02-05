use crate::app::App;
use crate::store::UpsertMessageParams;
use anyhow::Result;
use chrono::{DateTime, Utc};
use grammers_client::types::{Peer, Update};
use grammers_client::UpdatesConfiguration;
use grammers_session::defs::PeerRef;
use std::collections::HashSet;
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
    #[allow(dead_code)]
    pub download_media: bool,
    pub enable_socket: bool,
    pub idle_exit_secs: u64,
    pub ignore_chat_ids: Vec<i64>,
    pub ignore_channels: bool,
}

pub struct SyncResult {
    pub messages_stored: u64,
    pub chats_stored: u64,
}

impl App {
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

        // Phase 1: Bootstrap — fetch recent dialogs and their messages
        eprintln!("Bootstrapping: fetching dialogs…");
        let mut dialogs = client.iter_dialogs();
        while let Some(dialog) = dialogs.next().await? {
            let peer = dialog.peer();
            let (kind, name, username) = peer_info(peer);
            let id = peer.id().bare_id();

            // Skip ignored chats.
            if should_ignore(id, &kind) {
                continue;
            }

            self.store
                .upsert_chat(id, &kind, &name, username.as_deref(), None)
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

            while let Some(msg) = message_iter.next().await? {
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
                let media_type = msg.media().map(|_m| "media".to_string());
                let reply_to_id = msg.reply_to_message_id().map(|id| id as i64);

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
                        reply_to_id,
                    })
                    .await?;
                messages_stored += 1;

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
                    .upsert_chat(id, &kind, &name, username.as_deref(), Some(ts))
                    .await?;
            }
        }

        eprintln!(
            "Bootstrap complete: {} chats, {} messages",
            chats_stored, messages_stored
        );

        // Phase 2: Follow mode — listen for updates
        if matches!(opts.mode, SyncMode::Follow) {
            eprintln!("Listening for updates (Ctrl+C to stop)…");

            // Start socket server if requested
            let _socket_handle = if opts.enable_socket {
                let store_dir = self.store_dir.clone();
                Some(tokio::spawn(async move {
                    if let Err(e) = crate::app::socket::run_server(&store_dir).await {
                        log::error!("Socket server error: {}", e);
                    }
                }))
            } else {
                None
            };

            // Take ownership of updates receiver
            if let Some(updates_rx) = self.updates_rx.take() {
                let mut update_stream =
                    client.stream_updates(updates_rx, UpdatesConfiguration::default());

                let idle_duration = Duration::from_secs(opts.idle_exit_secs);
                let mut last_activity = std::time::Instant::now();

                loop {
                    let update = tokio::time::timeout(idle_duration, update_stream.next()).await;

                    match update {
                        Ok(Ok(update)) => {
                            last_activity = std::time::Instant::now();
                            match update {
                                Update::NewMessage(msg) if !msg.outgoing() => {
                                    let chat_peer = msg.peer();
                                    let (kind, name, username) = match &chat_peer {
                                        Ok(p) => peer_info(p),
                                        Err(_) => ("unknown".to_string(), "".to_string(), None),
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

                                    self.store
                                        .upsert_chat(
                                            chat_id,
                                            &kind,
                                            &name,
                                            username.as_deref(),
                                            Some(msg_ts),
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
                                            media_type: msg.media().map(|_| "media".to_string()),
                                            reply_to_id: msg
                                                .reply_to_message_id()
                                                .map(|id| id as i64),
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
                                    let (kind, _, _) = match msg.peer() {
                                        Ok(p) => peer_info(&p),
                                        Err(_) => ("unknown".to_string(), "".to_string(), None),
                                    };

                                    // Skip ignored chats.
                                    if should_ignore(chat_id, &kind) {
                                        continue;
                                    }

                                    let msg_ts = msg.date();

                                    self.store
                                        .upsert_message(UpsertMessageParams {
                                            id: msg.id() as i64,
                                            chat_id,
                                            sender_id: 0,
                                            ts: msg_ts,
                                            edit_ts: msg.edit_date(),
                                            from_me: true,
                                            text: msg.text().to_string(),
                                            media_type: msg.media().map(|_| "media".to_string()),
                                            reply_to_id: msg
                                                .reply_to_message_id()
                                                .map(|id| id as i64),
                                        })
                                        .await?;
                                    messages_stored += 1;
                                }
                                Update::MessageEdited(msg) => {
                                    let chat_id = msg.peer_id().bare_id();
                                    let (kind, _, _) = match msg.peer() {
                                        Ok(p) => peer_info(&p),
                                        Err(_) => ("unknown".to_string(), "".to_string(), None),
                                    };

                                    // Skip ignored chats.
                                    if should_ignore(chat_id, &kind) {
                                        continue;
                                    }

                                    let msg_ts = msg.date();

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
                                            media_type: msg.media().map(|_| "media".to_string()),
                                            reply_to_id: msg
                                                .reply_to_message_id()
                                                .map(|id| id as i64),
                                        })
                                        .await?;
                                }
                                _ => {}
                            }
                        }
                        Ok(Err(e)) => {
                            log::error!("Update error: {}", e);
                            break;
                        }
                        Err(_) => {
                            // Timeout
                            if matches!(opts.mode, SyncMode::Once) {
                                if last_activity.elapsed() >= idle_duration {
                                    eprintln!("Idle timeout reached, exiting.");
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
}

fn peer_info(peer: &Peer) -> (String, String, Option<String>) {
    match peer {
        Peer::User(user) => {
            let name = user.full_name();
            let username = user.username().map(|s| s.to_string());
            ("user".to_string(), name, username)
        }
        Peer::Group(group) => {
            let name = group.title().map(|s| s.to_string()).unwrap_or_default();
            let username = group.username().map(|s| s.to_string());
            ("group".to_string(), name, username)
        }
        Peer::Channel(channel) => {
            let name = channel.title().to_string();
            let username = channel.username().map(|s| s.to_string());
            ("channel".to_string(), name, username)
        }
    }
}
