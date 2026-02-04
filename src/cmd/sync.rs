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

    /// Keep syncing (daemon mode)
    #[arg(long, default_value_t = false)]
    pub follow: bool,

    /// Download media files
    #[arg(long, default_value_t = false)]
    pub download_media: bool,

    /// Automatically mark incoming messages as read
    #[arg(long, default_value_t = false)]
    pub mark_read: bool,

    /// Output mode: none, text, json
    #[arg(long, default_value = "none")]
    pub output: String,

    /// Enable Unix socket for IPC
    #[arg(long, default_value_t = false)]
    pub socket: bool,

    /// Idle exit timeout in seconds (for --once mode)
    #[arg(long, default_value = "30")]
    pub idle_exit: u64,
}

pub async fn run(cli: &Cli, args: &SyncArgs) -> Result<()> {
    let mut app = App::new(cli).await?;

    let mode = if args.follow {
        crate::app::sync::SyncMode::Follow
    } else if args.once {
        crate::app::sync::SyncMode::Once
    } else {
        // Default to once
        crate::app::sync::SyncMode::Once
    };

    let output_mode = match args.output.as_str() {
        "text" => crate::app::sync::OutputMode::Text,
        "json" => crate::app::sync::OutputMode::Json,
        _ => crate::app::sync::OutputMode::None,
    };

    let opts = crate::app::sync::SyncOptions {
        mode,
        output: output_mode,
        mark_read: args.mark_read,
        download_media: args.download_media,
        enable_socket: args.socket,
        idle_exit_secs: args.idle_exit,
    };

    let result = app.sync(opts).await?;

    if cli.json {
        out::write_json(&serde_json::json!({
            "synced": true,
            "messages_stored": result.messages_stored,
            "chats_stored": result.chats_stored,
        }))?;
    } else {
        eprintln!(
            "Sync complete. Messages: {}, Chats: {}",
            result.messages_stored, result.chats_stored
        );
    }

    Ok(())
}
