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
}

pub async fn run(cli: &Cli, cmd: &ChatsCommand) -> Result<()> {
    let store = Store::open(&cli.store_dir())?;

    match cmd {
        ChatsCommand::List { query, limit } => {
            let chats = store.list_chats(query.as_deref(), *limit)?;

            if cli.json {
                out::write_json(&chats)?;
            } else {
                println!(
                    "{:<6} {:<30} {:<16} {}",
                    "KIND", "NAME", "ID", "LAST MESSAGE"
                );
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
            let chat = store.get_chat(*id)?;
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
    }
    Ok(())
}
