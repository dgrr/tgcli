use crate::app::App;
use crate::out;
use crate::Cli;
use anyhow::Result;
use clap::{Args, Subcommand};

/// Common flags for all sync operations
#[derive(Args, Debug, Clone)]
pub struct CommonSyncArgs {
    /// Output mode for messages: none (default, summary only), text, or json
    #[arg(long, value_enum, default_value = "none")]
    pub output: out::OutputMode,

    /// Download media files
    #[arg(long, default_value_t = false)]
    pub download_media: bool,

    /// Automatically mark incoming messages as read
    #[arg(long, default_value_t = false)]
    pub mark_read: bool,

    /// Stream messages as JSONL (one JSON object per line)
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

    /// Number of concurrent chat syncs (default: 4)
    #[arg(long, default_value = "4")]
    pub concurrency: usize,

    /// Suppress summary output (just show "Sync complete")
    #[arg(long, default_value_t = false)]
    pub quiet: bool,

    /// After sync, prune messages keeping only the N most recent per chat
    #[arg(long, value_name = "N")]
    pub prune_after: Option<usize>,

    /// Skip archived chats entirely (don't fetch dialogs or messages from archived folder)
    #[arg(long, default_value_t = false)]
    pub skip_archived: bool,

    /// Sync ONLY archived chats (opposite of --skip-archived)
    #[arg(long, default_value_t = false, conflicts_with = "skip_archived")]
    pub archived_only: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub enum SyncCommand {
    /// Sync only chat list from Telegram dialogs (no messages)
    Chats {
        #[command(flatten)]
        common: CommonSyncArgs,
    },
    /// Sync only messages from existing local chats (uses stored access_hash)
    Msgs {
        #[command(flatten)]
        common: CommonSyncArgs,

        /// Sync only this specific chat (by chat ID)
        #[arg(long, value_name = "CHAT_ID")]
        chat: Option<i64>,
    },
}

#[derive(Args, Debug, Clone)]
pub struct SyncArgs {
    #[command(subcommand)]
    pub command: Option<SyncCommand>,

    #[command(flatten)]
    pub common: CommonSyncArgs,
}

fn build_output_mode(common: &CommonSyncArgs) -> crate::app::sync::OutputMode {
    if common.stream {
        crate::app::sync::OutputMode::Stream
    } else {
        // Use sync-specific --output flag (defaults to none for summary-only output)
        match common.output {
            crate::out::OutputMode::Json => crate::app::sync::OutputMode::Json,
            crate::out::OutputMode::Text => crate::app::sync::OutputMode::Text,
            crate::out::OutputMode::Markdown => crate::app::sync::OutputMode::Text, // Markdown falls back to text for sync
            crate::out::OutputMode::None => crate::app::sync::OutputMode::None,
        }
    }
}

fn build_sync_options(common: &CommonSyncArgs) -> crate::app::sync::SyncOptions {
    let output_mode = build_output_mode(common);

    crate::app::sync::SyncOptions {
        output: output_mode,
        mark_read: common.mark_read,
        download_media: common.download_media,
        ignore_chat_ids: common.ignore_chat_ids.clone(),
        ignore_channels: common.ignore_channels,
        show_progress: !common.no_progress,
        incremental: true, // Always incremental
        messages_per_chat: common.messages_per_chat,
        concurrency: common.concurrency,
        chat_filter: None,
        prune_after: common.prune_after,
        skip_archived: common.skip_archived,
        archived_only: common.archived_only,
    }
}

fn print_sync_result(
    common: &CommonSyncArgs,
    result: &crate::app::sync::SyncResult,
    mode_str: &str,
) {
    if common.output.is_json() {
        out::write_json(&serde_json::json!({
            "synced": true,
            "messages_stored": result.messages_stored,
            "chats_stored": result.chats_stored,
            "mode": mode_str,
            "per_chat": result.per_chat,
        }))
        .ok();
    } else if common.quiet {
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

        if chats_with_messages.is_empty() && result.chats_stored == 0 {
            eprintln!("Nothing synced.");
        } else if chats_with_messages.is_empty() {
            eprintln!("Synced {} chats (no new messages).", result.chats_stored);
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
                // Forum parent: show messages only (no unread)
                // Regular chat: show messages + unread
                if !chat.topics.is_empty() {
                    // Forum parent line - no unread count
                    println!(
                        "  {:<width$}  +{} {}",
                        chat.chat_name,
                        chat.messages_synced,
                        plural(chat.messages_synced),
                        width = max_name_len
                    );
                } else {
                    // Regular chat - show unread
                    println!(
                        "  {:<width$}  +{} {} +{} unread",
                        chat.chat_name,
                        chat.messages_synced,
                        plural(chat.messages_synced),
                        chat.unread_count,
                        width = max_name_len
                    );
                }
                // Show topic breakdown for forums - with unread count
                for topic in &chat.topics {
                    if topic.messages_synced > 0 {
                        println!(
                            "    └ {:<width$}  +{} {} +{} unread",
                            topic.topic_name,
                            topic.messages_synced,
                            plural(topic.messages_synced),
                            topic.unread_count,
                            width = max_name_len - 4
                        );
                    }
                }
            }
        }
    }
}

pub async fn run(cli: &Cli, args: &SyncArgs) -> Result<()> {
    match &args.command {
        Some(SyncCommand::Chats { common }) => {
            // Sync chats only (no messages)
            let mut app = App::new(cli).await?;
            let opts = build_sync_options(common);
            let result = app.sync_chats(opts).await?;
            print_sync_result(common, &result, "chats-only");
        }
        Some(SyncCommand::Msgs { common, chat }) => {
            // Sync messages only from local chats (uses stored access_hash, no iter_dialogs)
            let mut app = App::new(cli).await?;
            let mut opts = build_sync_options(common);
            opts.chat_filter = *chat;
            let result = app.sync_msgs(opts).await?;
            print_sync_result(common, &result, "msgs-only");
        }
        None => {
            // Default: sync both chats and messages
            let mut app = App::new(cli).await?;
            let opts = build_sync_options(&args.common);
            let result = app.sync(opts).await?;
            print_sync_result(&args.common, &result, "incremental");
        }
    }

    Ok(())
}
