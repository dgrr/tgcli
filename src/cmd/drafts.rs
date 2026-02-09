use crate::app::App;
use crate::out;
use crate::out::markdown::{format_drafts, DraftMd};
use crate::store::Store;
use crate::Cli;
use anyhow::Result;
use clap::Subcommand;
use serde::Serialize;

#[derive(Subcommand, Debug, Clone)]
pub enum DraftsCommand {
    /// List drafts across all chats
    List {
        /// Limit results
        #[arg(long, default_value = "50")]
        limit: usize,
    },
    /// Clear draft for a specific chat
    Clear {
        /// Chat ID to clear draft from
        #[arg(long)]
        chat: i64,
    },
}

#[derive(Debug, Clone, Serialize)]
struct DraftInfo {
    chat_id: i64,
    chat_name: Option<String>,
    text: String,
    date: String,
    reply_to_msg_id: Option<i32>,
}

pub async fn run(cli: &Cli, cmd: &DraftsCommand) -> Result<()> {
    let store = Store::open(&cli.store_dir()).await?;

    match cmd {
        DraftsCommand::List { limit } => {
            let app = App::new(cli).await?;
            let drafts = app.list_drafts(*limit).await?;

            // Enrich with chat names from local store
            let mut enriched: Vec<DraftInfo> = Vec::new();
            for draft in drafts {
                let chat = store.get_chat(draft.chat_id).await?;
                enriched.push(DraftInfo {
                    chat_id: draft.chat_id,
                    chat_name: chat.map(|c| c.name),
                    text: draft.text,
                    date: draft.date,
                    reply_to_msg_id: draft.reply_to_msg_id,
                });
            }

            if cli.output.is_json() {
                out::write_json(&serde_json::json!({
                    "count": enriched.len(),
                    "drafts": enriched,
                }))?;
            } else if cli.output.is_markdown() {
                let drafts_md: Vec<DraftMd> = enriched.iter().map(|d| DraftMd {
                    chat_id: d.chat_id,
                    chat_name: d.chat_name.clone(),
                    text: d.text.clone(),
                    date: d.date.clone(),
                    reply_to_msg_id: d.reply_to_msg_id,
                }).collect();
                out::write_markdown(&format_drafts(&drafts_md));
            } else if enriched.is_empty() {
                println!("No drafts found.");
            } else {
                println!("{:<16} {:<30} {:<20} TEXT", "CHAT_ID", "CHAT_NAME", "DATE");
                for d in &enriched {
                    let chat_name = d.chat_name.as_deref().unwrap_or("-");
                    let text_preview = out::truncate(&d.text.replace('\n', " "), 50);
                    println!(
                        "{:<16} {:<30} {:<20} {}",
                        d.chat_id,
                        out::truncate(chat_name, 28),
                        &d.date[..std::cmp::min(19, d.date.len())],
                        text_preview
                    );
                }
            }
        }
        DraftsCommand::Clear { chat } => {
            let app = App::new(cli).await?;

            // Get chat info for display
            let chat_info = store.get_chat(*chat).await?;
            let chat_name = chat_info
                .as_ref()
                .map(|c| c.name.clone())
                .unwrap_or_else(|| format!("Chat {}", chat));

            app.clear_draft(*chat).await?;

            if cli.output.is_json() {
                out::write_json(&serde_json::json!({
                    "cleared": true,
                    "chat_id": chat,
                    "chat_name": chat_name,
                }))?;
            } else {
                println!("Cleared draft for \"{}\" ({})", chat_name, chat);
            }
        }
    }
    Ok(())
}
