use crate::out;
use crate::store::Store;
use crate::Cli;
use anyhow::Result;
use clap::Args;
use std::io::{self, Write};

#[derive(Args, Debug, Clone)]
pub struct ClearArgs {
    /// Only clear chats and messages
    #[arg(long)]
    pub chats: bool,

    /// Only clear contacts
    #[arg(long)]
    pub contacts: bool,

    /// Skip confirmation prompt
    #[arg(long, short = 'y')]
    pub confirm: bool,
}

pub async fn run(cli: &Cli, args: &ClearArgs) -> Result<()> {
    let store_dir = cli.store_dir();
    let store = Store::open(&store_dir).await?;

    // Determine what to clear
    let clear_all = !args.chats && !args.contacts;
    let clear_chats = clear_all || args.chats;
    let clear_contacts = clear_all || args.contacts;

    // Get counts before clearing
    let counts = get_counts(&store, clear_chats, clear_contacts).await?;

    if counts.total() == 0 {
        if cli.json {
            out::write_json(&serde_json::json!({
                "cleared": false,
                "reason": "nothing to clear"
            }))?;
        } else {
            println!("Nothing to clear.");
        }
        return Ok(());
    }

    // Show what will be deleted
    if !cli.json && !args.confirm {
        println!("This will delete:");
        if clear_chats {
            println!("  - {} messages", counts.messages);
            println!("  - {} chats", counts.chats);
            println!("  - {} topics", counts.topics);
        }
        if clear_contacts {
            println!("  - {} contacts", counts.contacts);
        }
        println!();
        print!("Are you sure? [y/N] ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim().to_lowercase();

        if input != "y" && input != "yes" {
            println!("Aborted.");
            return Ok(());
        }
    }

    // Perform the deletion
    let deleted = clear_tables(&store, clear_chats, clear_contacts).await?;

    if cli.json {
        out::write_json(&serde_json::json!({
            "cleared": true,
            "deleted": {
                "messages": deleted.messages,
                "chats": deleted.chats,
                "topics": deleted.topics,
                "contacts": deleted.contacts
            }
        }))?;
    } else {
        println!("Cleared:");
        if clear_chats {
            println!("  - {} messages", deleted.messages);
            println!("  - {} chats", deleted.chats);
            println!("  - {} topics", deleted.topics);
        }
        if clear_contacts {
            println!("  - {} contacts", deleted.contacts);
        }
    }

    Ok(())
}

#[derive(Default)]
struct Counts {
    messages: u64,
    chats: u64,
    topics: u64,
    contacts: u64,
}

impl Counts {
    fn total(&self) -> u64 {
        self.messages + self.chats + self.topics + self.contacts
    }
}

async fn get_counts(store: &Store, chats: bool, contacts: bool) -> Result<Counts> {
    let mut counts = Counts::default();

    if chats {
        counts.messages = store.count_messages().await?;
        counts.chats = store.count_chats().await?;
        counts.topics = store.count_topics().await?;
    }
    if contacts {
        counts.contacts = store.count_contacts().await?;
    }

    Ok(counts)
}

async fn clear_tables(store: &Store, chats: bool, contacts: bool) -> Result<Counts> {
    let mut deleted = Counts::default();

    if chats {
        deleted.messages = store.clear_messages().await?;
        deleted.topics = store.clear_topics().await?;
        deleted.chats = store.clear_chats().await?;
    }
    if contacts {
        deleted.contacts = store.clear_contacts().await?;
    }

    Ok(deleted)
}
