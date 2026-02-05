use crate::app::App;
use crate::out;
use crate::store::{self, Store};
use crate::Cli;
use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand, Debug, Clone)]
pub enum MessagesCommand {
    /// List messages
    List {
        /// Chat ID
        #[arg(long)]
        chat: Option<i64>,
        /// Limit results
        #[arg(long, default_value = "50")]
        limit: i64,
        /// Only messages after this time (RFC3339 or YYYY-MM-DD)
        #[arg(long)]
        after: Option<String>,
        /// Only messages before this time (RFC3339 or YYYY-MM-DD)
        #[arg(long)]
        before: Option<String>,
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
        /// Chat IDs to exclude (repeatable)
        #[arg(long = "ignore", value_name = "CHAT_ID")]
        ignore_chats: Vec<i64>,
        /// Exclude channels
        #[arg(long)]
        ignore_channels: bool,
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
}

pub async fn run(cli: &Cli, cmd: &MessagesCommand) -> Result<()> {
    let store = Store::open(&cli.store_dir()).await?;

    match cmd {
        MessagesCommand::List {
            chat,
            limit,
            after,
            before,
            ignore_chats,
            ignore_channels,
        } => {
            let after_ts = after.as_deref().map(parse_time).transpose()?;
            let before_ts = before.as_deref().map(parse_time).transpose()?;

            let msgs = store
                .list_messages(store::ListMessagesParams {
                    chat_id: *chat,
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
                    "{:<20} {:<24} {:<18} {:<10} {}",
                    "TIME", "CHAT", "FROM", "ID", "TEXT"
                );
                for m in &msgs {
                    let from = if m.from_me {
                        "me".to_string()
                    } else {
                        m.sender_id.to_string()
                    };
                    let text = out::truncate(&m.text, 80);
                    let ts = m.ts.format("%Y-%m-%d %H:%M:%S").to_string();
                    println!(
                        "{:<20} {:<24} {:<18} {:<10} {}",
                        ts,
                        out::truncate(&m.chat_id.to_string(), 22),
                        out::truncate(&from, 16),
                        m.id,
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
                    "{:<20} {:<24} {:<18} {:<10} {}",
                    "TIME", "CHAT", "FROM", "ID", "MATCH"
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
                    "{:<20} {:<24} {:<18} {:<10} {}",
                    "TIME", "CHAT", "FROM", "ID", "TEXT"
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
