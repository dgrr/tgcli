use crate::app::App;
use crate::out;
use crate::store::{self, Store};
use crate::Cli;
use anyhow::Result;
use clap::{Subcommand, ValueEnum};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ExportFormat {
    Json,
    Csv,
}

#[derive(Subcommand, Debug, Clone)]
pub enum MessagesCommand {
    /// Fetch older messages from Telegram (backfill history)
    Fetch {
        /// Chat ID (required)
        #[arg(long)]
        chat: i64,
        /// Topic ID (for forum groups)
        #[arg(long)]
        topic: Option<i32>,
        /// Number of messages to fetch
        #[arg(long, default_value = "100")]
        limit: usize,
    },
    /// List messages
    List {
        /// Chat ID
        #[arg(long)]
        chat: Option<i64>,
        /// Topic ID (for forum groups)
        #[arg(long)]
        topic: Option<i32>,
        /// Limit results
        #[arg(long, default_value = "50")]
        limit: i64,
        /// Only messages after this time (RFC3339, YYYY-MM-DD, 'today', or 'yesterday')
        #[arg(long)]
        after: Option<String>,
        /// Only messages before this time (RFC3339, YYYY-MM-DD, 'today', or 'yesterday')
        #[arg(long)]
        before: Option<String>,
        /// Only messages from today
        #[arg(long)]
        today: bool,
        /// Only messages from yesterday
        #[arg(long)]
        yesterday: bool,
        /// Chat IDs to exclude (repeatable)
        #[arg(long = "ignore", value_name = "CHAT_ID")]
        ignore_chats: Vec<i64>,
        /// Exclude channels
        #[arg(long)]
        ignore_channels: bool,
    },
    /// Search messages (FTS5)
    Search {
        /// Search query
        query: String,
        /// Chat ID filter
        #[arg(long)]
        chat: Option<i64>,
        /// Sender ID filter
        #[arg(long)]
        from: Option<i64>,
        /// Limit results
        #[arg(long, default_value = "50")]
        limit: i64,
        /// Media type filter
        #[arg(long, name = "type")]
        media_type: Option<String>,
        /// Only messages after this time (RFC3339, YYYY-MM-DD, 'today', or 'yesterday')
        #[arg(long)]
        after: Option<String>,
        /// Only messages before this time (RFC3339, YYYY-MM-DD, 'today', or 'yesterday')
        #[arg(long)]
        before: Option<String>,
        /// Only messages from today
        #[arg(long)]
        today: bool,
        /// Only messages from yesterday
        #[arg(long)]
        yesterday: bool,
        /// Chat IDs to exclude (repeatable)
        #[arg(long = "ignore", value_name = "CHAT_ID")]
        ignore_chats: Vec<i64>,
        /// Exclude channels
        #[arg(long)]
        ignore_channels: bool,
    },
    /// Export messages to stdout (JSON or CSV)
    Export {
        /// Chat ID (required)
        #[arg(long)]
        chat: i64,
        /// Output format
        #[arg(long, value_enum, default_value = "json")]
        format: ExportFormat,
        /// Maximum messages to export (default: all)
        #[arg(long)]
        limit: Option<i64>,
        /// Only messages after this time
        #[arg(long)]
        after: Option<String>,
        /// Only messages before this time
        #[arg(long)]
        before: Option<String>,
        /// Only messages from today
        #[arg(long)]
        today: bool,
        /// Only messages from yesterday
        #[arg(long)]
        yesterday: bool,
    },
    /// Show message context around a message
    Context {
        /// Chat ID
        #[arg(long)]
        chat: i64,
        /// Message ID
        #[arg(long)]
        id: i64,
        /// Messages before
        #[arg(long, default_value = "5")]
        before: i64,
        /// Messages after
        #[arg(long, default_value = "5")]
        after: i64,
    },
    /// Show a single message
    Show {
        /// Chat ID
        #[arg(long)]
        chat: i64,
        /// Message ID
        #[arg(long)]
        id: i64,
    },
    /// Delete messages from a chat (always deletes for everyone)
    Delete {
        /// Chat ID
        #[arg(long)]
        chat: i64,
        /// Message ID(s) to delete (repeatable)
        #[arg(long = "id", value_name = "MSG_ID")]
        ids: Vec<i64>,
    },
    /// Forward a message to another chat
    Forward {
        /// Source chat ID
        #[arg(long)]
        chat: i64,
        /// Message ID to forward
        #[arg(long)]
        id: i64,
        /// Destination chat ID
        #[arg(long)]
        to: i64,
    },
    /// Edit a message's text
    Edit {
        /// Chat ID
        #[arg(long)]
        chat: i64,
        /// Message ID to edit
        #[arg(long)]
        id: i64,
        /// New message text
        #[arg(long)]
        text: String,
    },
    /// Pin a message in a chat
    Pin {
        /// Chat ID
        #[arg(long)]
        chat: i64,
        /// Message ID to pin
        #[arg(long)]
        id: i64,
        /// Pin silently (no notification)
        #[arg(long)]
        silent: bool,
        /// Pin only for yourself (not visible to others)
        #[arg(long)]
        pm_oneside: bool,
    },
    /// Unpin a message in a chat
    Unpin {
        /// Chat ID
        #[arg(long)]
        chat: i64,
        /// Message ID to unpin
        #[arg(long)]
        id: i64,
        /// Unpin only for yourself
        #[arg(long)]
        pm_oneside: bool,
    },
    /// Add or remove a reaction from a message
    React {
        /// Chat ID
        #[arg(long)]
        chat: i64,
        /// Message ID to react to
        #[arg(long, name = "message")]
        msg_id: i64,
        /// Emoji reaction (e.g., "👍", "❤️", "🔥")
        #[arg(long)]
        emoji: String,
        /// Remove the reaction instead of adding it
        #[arg(long)]
        remove: bool,
    },
}

pub async fn run(cli: &Cli, cmd: &MessagesCommand) -> Result<()> {
    let store = Store::open(&cli.store_dir()).await?;

    match cmd {
        MessagesCommand::Fetch { chat, topic, limit } => {
            // Get oldest message ID we have for this chat
            let oldest_id = store.get_oldest_message_id(*chat, *topic).await?;

            // Requires network access
            let app = App::new(cli).await?;

            let fetched = app
                .backfill_messages(*chat, *topic, oldest_id, *limit)
                .await?;

            if cli.json {
                out::write_json(&serde_json::json!({
                    "chat_id": chat,
                    "topic_id": topic,
                    "offset_id": oldest_id,
                    "fetched": fetched,
                }))?;
            } else {
                if let Some(oid) = oldest_id {
                    println!(
                        "Fetched {} messages older than ID {} from chat {}",
                        fetched, oid, chat
                    );
                } else {
                    println!(
                        "Fetched {} messages from chat {} (no prior messages)",
                        fetched, chat
                    );
                }
                if let Some(tid) = topic {
                    println!("  (topic: {})", tid);
                }
            }
        }
        MessagesCommand::List {
            chat,
            topic,
            limit,
            after,
            before,
            ignore_chats,
            ignore_channels,
            ..
        } => {
            let after_ts = after.as_deref().map(parse_time).transpose()?;
            let before_ts = before.as_deref().map(parse_time).transpose()?;

            let msgs = store
                .list_messages(store::ListMessagesParams {
                    chat_id: *chat,
                    topic_id: *topic,
                    limit: *limit,
                    after: after_ts,
                    before: before_ts,
                    ignore_chats: ignore_chats.clone(),
                    ignore_channels: *ignore_channels,
                })
                .await?;

            if cli.json {
                out::write_json(&serde_json::json!({
                    "messages": msgs,
                }))?;
            } else {
                println!(
                    "{:<20} {:<24} {:<18} {:<10} {:<8} TEXT",
                    "TIME", "CHAT", "FROM", "ID", "TOPIC"
                );
                for m in &msgs {
                    let from = if m.from_me {
                        "me".to_string()
                    } else {
                        m.sender_id.to_string()
                    };
                    let topic_str = m
                        .topic_id
                        .map(|t| t.to_string())
                        .unwrap_or_else(|| "-".to_string());
                    let text = out::truncate(&m.text, 70);
                    let ts = m.ts.format("%Y-%m-%d %H:%M:%S").to_string();
                    println!(
                        "{:<20} {:<24} {:<18} {:<10} {:<8} {}",
                        ts,
                        out::truncate(&m.chat_id.to_string(), 22),
                        out::truncate(&from, 16),
                        m.id,
                        topic_str,
                        text,
                    );
                }
            }
        }
        MessagesCommand::Search {
            query,
            chat,
            from,
            limit,
            media_type,
            ignore_chats,
            ignore_channels,
            ..
        } => {
            let msgs = store
                .search_messages(store::SearchMessagesParams {
                    query: query.clone(),
                    chat_id: *chat,
                    from_id: *from,
                    limit: *limit,
                    media_type: media_type.clone(),
                    ignore_chats: ignore_chats.clone(),
                    ignore_channels: *ignore_channels,
                })
                .await?;

            if cli.json {
                out::write_json(&serde_json::json!({
                    "messages": msgs,
                    "fts": store.has_fts(),
                }))?;
            } else {
                println!(
                    "{:<20} {:<24} {:<18} {:<10} MATCH",
                    "TIME", "CHAT", "FROM", "ID"
                );
                for m in &msgs {
                    let from = if m.from_me {
                        "me".to_string()
                    } else {
                        m.sender_id.to_string()
                    };
                    let text = if !m.snippet.is_empty() {
                        &m.snippet
                    } else {
                        &m.text
                    };
                    let ts = m.ts.format("%Y-%m-%d %H:%M:%S").to_string();
                    println!(
                        "{:<20} {:<24} {:<18} {:<10} {}",
                        ts,
                        out::truncate(&m.chat_id.to_string(), 22),
                        out::truncate(&from, 16),
                        m.id,
                        out::truncate(text, 90),
                    );
                }
                if !store.has_fts() {
                    eprintln!("Note: FTS5 not enabled; search is using LIKE (slow).");
                }
            }
        }
        MessagesCommand::Context {
            chat,
            id,
            before,
            after,
        } => {
            let msgs = store.message_context(*chat, *id, *before, *after).await?;

            if cli.json {
                out::write_json(&msgs)?;
            } else {
                println!(
                    "{:<20} {:<24} {:<18} {:<10} TEXT",
                    "TIME", "CHAT", "FROM", "ID"
                );
                for m in &msgs {
                    let from = if m.from_me {
                        "me".to_string()
                    } else {
                        m.sender_id.to_string()
                    };
                    let prefix = if m.id == *id { ">> " } else { "" };
                    let ts = m.ts.format("%Y-%m-%d %H:%M:%S").to_string();
                    println!(
                        "{:<20} {:<24} {:<18} {:<10} {}{}",
                        ts,
                        out::truncate(&m.chat_id.to_string(), 22),
                        out::truncate(&from, 16),
                        m.id,
                        prefix,
                        out::truncate(&m.text, 80),
                    );
                }
            }
        }
        MessagesCommand::Show { chat, id } => {
            let msg = store.get_message(*chat, *id).await?;
            match msg {
                Some(m) => {
                    if cli.json {
                        out::write_json(&m)?;
                    } else {
                        println!("Chat: {}", m.chat_id);
                        println!("ID: {}", m.id);
                        println!("Time: {}", m.ts.to_rfc3339());
                        if m.from_me {
                            println!("From: me");
                        } else {
                            println!("From: {}", m.sender_id);
                        }
                        if let Some(topic_id) = m.topic_id {
                            println!("Topic: {}", topic_id);
                        }
                        if let Some(ref mt) = m.media_type {
                            println!("Media: {}", mt);
                        }
                        println!();
                        println!("{}", m.text);
                    }
                }
                None => {
                    anyhow::bail!("Message {}/{} not found", chat, id);
                }
            }
        }
        MessagesCommand::Delete { chat, ids } => {
            if ids.is_empty() {
                anyhow::bail!("At least one --id is required");
            }

            // Delete requires network access
            let app = App::new(cli).await?;

            let deleted = app.delete_messages(*chat, ids).await?;

            if cli.json {
                out::write_json(&serde_json::json!({
                    "deleted": true,
                    "chat_id": chat,
                    "message_ids": ids,
                    "affected_count": deleted,
                }))?;
            } else {
                println!(
                    "Deleted {} message(s) from chat {} (affected: {})",
                    ids.len(),
                    chat,
                    deleted
                );
            }
        }
        MessagesCommand::Forward { chat, id, to } => {
            // Forward requires network access
            let app = App::new(cli).await?;

            let new_msg_id = app.forward_message(*chat, *id, *to).await?;

            if cli.json {
                out::write_json(&serde_json::json!({
                    "forwarded": true,
                    "from_chat": chat,
                    "message_id": id,
                    "to_chat": to,
                    "new_message_id": new_msg_id,
                }))?;
            } else {
                println!(
                    "Forwarded message {} from {} to {} (new ID: {})",
                    id, chat, to, new_msg_id
                );
            }
        }
        MessagesCommand::Edit { chat, id, text } => {
            // Edit requires network access
            let app = App::new(cli).await?;

            app.edit_message(*chat, *id, text).await?;

            if cli.json {
                out::write_json(&serde_json::json!({
                    "edited": true,
                    "chat_id": chat,
                    "message_id": id,
                }))?;
            } else {
                println!("Edited message {} in chat {}", id, chat);
            }
        }
        MessagesCommand::Pin {
            chat,
            id,
            silent,
            pm_oneside,
        } => {
            // Pin requires network access
            let app = App::new(cli).await?;

            app.pin_message(*chat, *id, *silent, *pm_oneside).await?;

            if cli.json {
                out::write_json(&serde_json::json!({
                    "pinned": true,
                    "chat_id": chat,
                    "message_id": id,
                }))?;
            } else {
                println!("Pinned message {} in chat {}", id, chat);
            }
        }
        MessagesCommand::Unpin {
            chat,
            id,
            pm_oneside,
        } => {
            // Unpin requires network access
            let app = App::new(cli).await?;

            app.unpin_message(*chat, *id, *pm_oneside).await?;

            if cli.json {
                out::write_json(&serde_json::json!({
                    "unpinned": true,
                    "chat_id": chat,
                    "message_id": id,
                }))?;
            } else {
                println!("Unpinned message {} in chat {}", id, chat);
            }
        }
        MessagesCommand::Export { .. } => {
            anyhow::bail!("Export command is not yet implemented");
        }
        MessagesCommand::React {
            chat,
            msg_id,
            emoji,
            remove,
        } => {
            // React requires network access
            let app = App::new(cli).await?;

            app.send_reaction(*chat, *msg_id, emoji, *remove).await?;

            if cli.json {
                out::write_json(&serde_json::json!({
                    "success": true,
                    "chat_id": chat,
                    "message_id": msg_id,
                    "emoji": emoji,
                    "removed": remove,
                }))?;
            } else {
                if *remove {
                    println!(
                        "Removed reaction {} from message {} in chat {}",
                        emoji, msg_id, chat
                    );
                } else {
                    println!(
                        "Added reaction {} to message {} in chat {}",
                        emoji, msg_id, chat
                    );
                }
            }
        }
    }
    Ok(())
}

fn parse_time(s: &str) -> Result<chrono::DateTime<chrono::Utc>> {
    // Try RFC3339 first
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&chrono::Utc));
    }
    // Try YYYY-MM-DD
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let dt = d.and_hms_opt(0, 0, 0).unwrap().and_utc();
        return Ok(dt);
    }
    anyhow::bail!("Invalid time format: {} (use RFC3339 or YYYY-MM-DD)", s);
}
