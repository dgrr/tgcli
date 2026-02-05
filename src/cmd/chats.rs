use crate::out;
use crate::store::Store;
use crate::Cli;
use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand, Debug, Clone)]
pub enum ChatsCommand {
    /// List chats
    List {
        /// Search query
        #[arg(long)]
        query: Option<String>,
        /// Limit results
        #[arg(long, default_value = "50")]
        limit: i64,
    },
    /// Show a single chat
    Show {
        /// Chat ID
        #[arg(long)]
        id: i64,
    },
    /// Delete a chat from local database
    Delete {
        /// Chat ID to delete
        chat_id: i64,
        /// Soft delete (local DB only, default behavior)
        #[arg(long, default_value = "true")]
        soft: bool,
        /// Hard delete (also delete from Telegram) - NOT IMPLEMENTED
        #[arg(long, conflicts_with = "soft")]
        hard: bool,
    },
}

pub async fn run(cli: &Cli, cmd: &ChatsCommand) -> Result<()> {
    let store = Store::open(&cli.store_dir()).await?;

    match cmd {
        ChatsCommand::List { query, limit } => {
            let chats = store.list_chats(query.as_deref(), *limit).await?;

            if cli.json {
                out::write_json(&chats)?;
            } else {
                println!("{:<6} {:<30} {:<16} LAST MESSAGE", "KIND", "NAME", "ID");
                for c in &chats {
                    let name = out::truncate(&c.name, 28);
                    let ts = c
                        .last_message_ts
                        .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_default();
                    println!("{:<6} {:<30} {:<16} {}", c.kind, name, c.id, ts);
                }
            }
        }
        ChatsCommand::Show { id } => {
            let chat = store.get_chat(*id).await?;
            match chat {
                Some(c) => {
                    if cli.json {
                        out::write_json(&c)?;
                    } else {
                        println!("ID: {}", c.id);
                        println!("Kind: {}", c.kind);
                        println!("Name: {}", c.name);
                        if let Some(u) = &c.username {
                            println!("Username: @{}", u);
                        }
                        if let Some(ts) = c.last_message_ts {
                            println!("Last message: {}", ts.to_rfc3339());
                        }
                    }
                }
                None => {
                    anyhow::bail!("Chat {} not found", id);
                }
            }
        }
        ChatsCommand::Delete {
            chat_id,
            soft: _,
            hard,
        } => {
            if *hard {
                anyhow::bail!("Hard delete not implemented. Use --soft (default) to delete from local DB only.");
            }

            // Get chat info before deletion for confirmation message
            let chat = store.get_chat(*chat_id).await?;
            let chat_name = chat
                .as_ref()
                .map(|c| c.name.clone())
                .unwrap_or_else(|| format!("(unknown chat {})", chat_id));

            // Delete messages first, then the chat
            let messages_deleted = store.delete_messages_by_chat(*chat_id).await?;
            let chat_deleted = store.delete_chat(*chat_id).await?;

            if cli.json {
                out::write_json(&serde_json::json!({
                    "chat_id": chat_id,
                    "chat_name": chat_name,
                    "chat_deleted": chat_deleted,
                    "messages_deleted": messages_deleted,
                }))?;
            } else if chat_deleted {
                println!("Deleted chat \"{}\" ({})", chat_name, chat_id);
                println!("Deleted {} message(s)", messages_deleted);
            } else {
                println!("Chat {} not found in local database", chat_id);
                if messages_deleted > 0 {
                    println!("Deleted {} orphaned message(s)", messages_deleted);
                }
            }
        }
    }
    Ok(())
}
