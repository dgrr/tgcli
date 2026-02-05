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

    /// Show summary of messages synced per chat (useful for LLMs)
    #[arg(long, default_value_t = false)]
    pub summary: bool,
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
    };

    let result = app.sync(opts).await?;

    if cli.json {
        out::write_json(&serde_json::json!({
            "synced": true,
            "messages_stored": result.messages_stored,
            "chats_stored": result.chats_stored,
            "incremental": incremental,
            "per_chat": result.per_chat,
        }))?;
    } else if args.summary {
        // Output summary for LLMs: chat_id and messages synced
        for chat in &result.per_chat {
            println!(
                "{}\t{}\t{}",
                chat.chat_id, chat.messages_synced, chat.chat_name
            );
        }
        eprintln!(
            "Total: {} messages across {} chats",
            result.messages_stored,
            result.per_chat.len()
        );
    } else {
        let mode_str = if incremental { "incremental" } else { "full" };
        eprintln!(
            "Sync complete ({}). Messages: {}, Chats: {}",
            mode_str, result.messages_stored, result.chats_stored
        );
    }

    Ok(())
}
