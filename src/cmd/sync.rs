use crate::app::App;
use crate::out;
use crate::Cli;
use anyhow::Result;
use clap::Args;

#[derive(Args, Debug, Clone)]
pub struct SyncArgs {
    /// Full sync: fetch all messages (default: incremental, only new messages)
    #[arg(long, default_value_t = false)]
    pub full: bool,

    /// Local-only sync: skip fetching dialogs from Telegram, only sync chats already in local DB
    #[arg(long, default_value_t = false)]
    pub local_only: bool,

    /// Download media files
    #[arg(long, default_value_t = false)]
    pub download_media: bool,

    /// Automatically mark incoming messages as read
    #[arg(long, default_value_t = false)]
    pub mark_read: bool,

    /// Output mode: none, text, json
    #[arg(long, default_value = "none")]
    pub output: String,

    /// Stream messages as JSONL (one JSON object per line, implies --output json)
    #[arg(long, default_value_t = false)]
    pub stream: bool,

    /// Chat IDs to ignore (skip during sync)
    #[arg(long = "ignore", value_name = "CHAT_ID")]
    pub ignore_chat_ids: Vec<i64>,

    /// Skip all channels
    #[arg(long, default_value_t = false)]
    pub ignore_channels: bool,

    /// Suppress progress output
    #[arg(long, default_value_t = false)]
    pub no_progress: bool,

    /// Maximum messages per chat during full sync (default: 50)
    #[arg(long, default_value = "50")]
    pub messages_per_chat: usize,

    /// Suppress summary output (just show "Sync complete")
    #[arg(long, default_value_t = false)]
    pub quiet: bool,
}

pub async fn run(cli: &Cli, args: &SyncArgs) -> Result<()> {
    let mut app = App::new(cli).await?;

    let output_mode = if args.stream {
        crate::app::sync::OutputMode::Stream
    } else {
        match args.output.as_str() {
            "text" => crate::app::sync::OutputMode::Text,
            "json" => crate::app::sync::OutputMode::Json,
            _ => crate::app::sync::OutputMode::None,
        }
    };

    // Default is incremental; --full overrides
    let incremental = !args.full;

    let opts = crate::app::sync::SyncOptions {
        output: output_mode,
        mark_read: args.mark_read,
        download_media: args.download_media,
        ignore_chat_ids: args.ignore_chat_ids.clone(),
        ignore_channels: args.ignore_channels,
        show_progress: !args.no_progress,
        incremental,
        messages_per_chat: args.messages_per_chat,
        local_only: args.local_only,
    };

    let result = app.sync(opts).await?;

    if cli.json {
        out::write_json(&serde_json::json!({
            "synced": true,
            "messages_stored": result.messages_stored,
            "chats_stored": result.chats_stored,
            "incremental": incremental,
            "local_only": args.local_only,
            "per_chat": result.per_chat,
        }))?;
    } else if args.quiet {
        let mode_str = if args.local_only {
            "local-only"
        } else if incremental {
            "incremental"
        } else {
            "full"
        };
        eprintln!(
            "Sync complete ({}). Messages: {}, Chats: {}",
            mode_str, result.messages_stored, result.chats_stored
        );
    } else {
        // Human-readable summary output
        let chats_with_messages: Vec<_> = result
            .per_chat
            .iter()
            .filter(|c| c.messages_synced > 0)
            .collect();

        if chats_with_messages.is_empty() {
            eprintln!("No new messages.");
        } else {
            // Calculate max name length for alignment (including topic indent)
            let mut max_name_len = 0;
            for chat in &chats_with_messages {
                max_name_len = max_name_len.max(chat.chat_name.len());
                for topic in &chat.topics {
                    // Topics get "  └ " prefix (4 chars) so add that to comparison
                    max_name_len = max_name_len.max(topic.topic_name.len() + 4);
                }
            }

            let plural = |n: u64| if n == 1 { "message" } else { "messages" };

            println!(
                "Synced {} {}:",
                chats_with_messages.len(),
                if chats_with_messages.len() == 1 {
                    "chat"
                } else {
                    "chats"
                }
            );

            for chat in &chats_with_messages {
                println!(
                    "  {:<width$}  +{} {}",
                    chat.chat_name,
                    chat.messages_synced,
                    plural(chat.messages_synced),
                    width = max_name_len
                );
                // Show topic breakdown for forums
                for topic in &chat.topics {
                    if topic.messages_synced > 0 {
                        println!(
                            "    └ {:<width$}  +{} {}",
                            topic.topic_name,
                            topic.messages_synced,
                            plural(topic.messages_synced),
                            width = max_name_len - 4
                        );
                    }
                }
            }
        }
    }

    Ok(())
}
