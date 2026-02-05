use crate::app::App;
use crate::out;
use crate::Cli;
use anyhow::Result;
use clap::Args;

#[derive(Args, Debug, Clone)]
pub struct SyncArgs {
    /// Sync once and exit after idle
    #[arg(long, default_value_t = false)]
    pub once: bool,

    /// Incremental sync: only fetch messages newer than last sync (default: true)
    #[arg(long, default_value_t = true)]
    pub incremental: bool,

    /// Full sync: fetch all messages regardless of last sync state
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

    /// Enable Unix socket for IPC
    #[arg(long, default_value_t = false)]
    pub socket: bool,

    /// Idle exit timeout in seconds (for --once mode)
    #[arg(long, default_value = "30")]
    pub idle_exit: u64,

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
}

pub async fn run(cli: &Cli, args: &SyncArgs) -> Result<()> {
    let mut app = App::new(cli).await?;

    let mode = crate::app::sync::SyncMode::Once;

    let output_mode = if args.stream {
        crate::app::sync::OutputMode::Stream
    } else {
        match args.output.as_str() {
            "text" => crate::app::sync::OutputMode::Text,
            "json" => crate::app::sync::OutputMode::Json,
            _ => crate::app::sync::OutputMode::None,
        }
    };

    // --full overrides --incremental
    let incremental = args.incremental && !args.full;

    let opts = crate::app::sync::SyncOptions {
        mode,
        output: output_mode,
        mark_read: args.mark_read,
        download_media: args.download_media,
        enable_socket: args.socket,
        idle_exit_secs: args.idle_exit,
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
        }))?;
    } else {
        let mode_str = if incremental { "incremental" } else { "full" };
        eprintln!(
            "Sync complete ({}). Messages: {}, Chats: {}",
            mode_str, result.messages_stored, result.chats_stored
        );
    }

    Ok(())
}
