use crate::store::Store;
use crate::Cli;
use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand, Debug, Clone)]
pub enum HookCommand {
    /// Add a webhook (replaces any existing one)
    Add {
        /// Webhook URL to POST to
        url: String,
        /// Prompt to include in webhook payload
        prompt: String,
        /// Only fire for this chat ID
        #[arg(long)]
        chat: Option<i64>,
    },
    /// Remove the current webhook
    Remove,
    /// Show the current webhook configuration
    Show,
}

pub async fn run(cli: &Cli, cmd: &HookCommand) -> Result<()> {
    let store_dir = cli.store_dir();
    let store = Store::open(&store_dir).await?;

    match cmd {
        HookCommand::Add { url, prompt, chat } => {
            store.set_webhook(url, prompt, *chat).await?;
            eprintln!("Webhook set: {}", url);
            if let Some(id) = chat {
                eprintln!("  Chat filter: {}", id);
            }
        }
        HookCommand::Remove => {
            if store.remove_webhook().await? {
                eprintln!("Webhook removed.");
            } else {
                eprintln!("No webhook configured.");
            }
        }
        HookCommand::Show => {
            if let Some(config) = store.get_webhook().await? {
                println!("URL:    {}", config.url);
                println!("Prompt: {}", config.prompt);
                if let Some(id) = config.chat_id {
                    println!("Chat:   {}", id);
                } else {
                    println!("Chat:   all");
                }
            } else {
                eprintln!("No webhook configured.");
            }
        }
    }

    Ok(())
}
