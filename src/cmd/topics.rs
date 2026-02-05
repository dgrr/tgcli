use crate::app::App;
use crate::out;
use crate::store::{self, Store};
use crate::Cli;
use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand, Debug, Clone)]
pub enum TopicsCommand {
    /// List topics in a forum group
    List {
        /// Chat ID (must be a forum group)
        #[arg(long)]
        chat: i64,
        /// Sync topics from Telegram before listing
        #[arg(long)]
        sync: bool,
    },
    /// Show messages from a specific topic
    Messages {
        /// Chat ID
        #[arg(long)]
        chat: i64,
        /// Topic ID
        #[arg(long)]
        topic: i32,
        /// Limit results
        #[arg(long, default_value = "50")]
        limit: i64,
        /// Only messages after this time (RFC3339 or YYYY-MM-DD)
        #[arg(long)]
        after: Option<String>,
        /// Only messages before this time (RFC3339 or YYYY-MM-DD)
        #[arg(long)]
        before: Option<String>,
    },
}

pub async fn run(cli: &Cli, cmd: &TopicsCommand) -> Result<()> {
    let store = Store::open(&cli.store_dir()).await?;

    match cmd {
        TopicsCommand::List { chat, sync } => {
            // Check if chat is a forum
            let chat_info = store.get_chat(*chat).await?;
            if let Some(ref c) = chat_info {
                if !c.is_forum {
                    anyhow::bail!("Chat {} ({}) is not a forum group", c.name, chat);
                }
            }

            // Sync topics from Telegram if requested
            if *sync {
                let app = App::new(cli).await?;
                let synced = app.sync_topics(*chat).await?;
                eprintln!("Synced {} topics from Telegram", synced);
            }

            let topics = store.list_topics(*chat).await?;

            if cli.output.is_json() {
                out::write_json(&serde_json::json!({
                    "chat_id": chat,
                    "topics": topics,
                }))?;
            } else {
                let chat_name = chat_info
                    .as_ref()
                    .map(|c| c.name.as_str())
                    .unwrap_or("Unknown");
                println!("Topics in \"{}\" ({}):\n", chat_name, chat);
                println!("{:<10} {:<30} COLOR", "ID", "NAME");
                for t in &topics {
                    let emoji = t.icon_emoji.as_deref().unwrap_or("");
                    println!(
                        "{:<10} {:<30} #{:06X} {}",
                        t.topic_id,
                        out::truncate(&t.name, 28),
                        t.icon_color,
                        emoji
                    );
                }
                if topics.is_empty() {
                    println!("(no topics found - try --sync to fetch from Telegram)");
                }
            }
        }
        TopicsCommand::Messages {
            chat,
            topic,
            limit,
            after,
            before,
        } => {
            let after_ts = after.as_deref().map(parse_time).transpose()?;
            let before_ts = before.as_deref().map(parse_time).transpose()?;

            // Get topic info for display
            let topic_info = store.get_topic(*chat, *topic).await?;
            let topic_name = topic_info
                .as_ref()
                .map(|t| t.name.as_str())
                .unwrap_or("Unknown");

            let msgs = store
                .list_messages(store::ListMessagesParams {
                    chat_id: Some(*chat),
                    topic_id: Some(*topic),
                    limit: *limit,
                    after: after_ts,
                    before: before_ts,
                    ignore_chats: Vec::new(),
                    ignore_channels: false,
                })
                .await?;

            if cli.output.is_json() {
                out::write_json(&serde_json::json!({
                    "chat_id": chat,
                    "topic_id": topic,
                    "topic_name": topic_name,
                    "messages": msgs,
                }))?;
            } else {
                println!(
                    "Messages in topic \"{}\" (id={}) of chat {}:\n",
                    topic_name, topic, chat
                );
                println!("{:<20} {:<18} {:<10} TEXT", "TIME", "FROM", "ID");
                for m in &msgs {
                    let from = if m.from_me {
                        "me".to_string()
                    } else {
                        m.sender_id.to_string()
                    };
                    let text = out::truncate(&m.text, 80);
                    let ts = m.ts.format("%Y-%m-%d %H:%M:%S").to_string();
                    println!(
                        "{:<20} {:<18} {:<10} {}",
                        ts,
                        out::truncate(&from, 16),
                        m.id,
                        text,
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
