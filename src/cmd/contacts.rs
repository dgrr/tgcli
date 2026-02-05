use crate::out;
use crate::store::Store;
use crate::Cli;
use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand, Debug, Clone)]
pub enum ContactsCommand {
    /// Search contacts
    Search {
        /// Search query
        #[arg(long)]
        query: String,
        /// Limit results
        #[arg(long, default_value = "50")]
        limit: i64,
    },
    /// Show a contact
    Show {
        /// User ID
        #[arg(long)]
        id: i64,
    },
}

pub async fn run(cli: &Cli, cmd: &ContactsCommand) -> Result<()> {
    let store = Store::open(&cli.store_dir()).await?;

    match cmd {
        ContactsCommand::Search { query, limit } => {
            let contacts = store.search_contacts(query, *limit).await?;

            if cli.json {
                out::write_json(&contacts)?;
            } else {
                println!(
                    "{:<16} {:<20} {:<20} {:<16} {}",
                    "ID", "FIRST", "LAST", "PHONE", "USERNAME"
                );
                for c in &contacts {
                    println!(
                        "{:<16} {:<20} {:<20} {:<16} {}",
                        c.user_id,
                        out::truncate(&c.first_name, 18),
                        out::truncate(&c.last_name, 18),
                        out::truncate(&c.phone, 14),
                        c.username.as_deref().unwrap_or(""),
                    );
                }
            }
        }
        ContactsCommand::Show { id } => {
            let contact = store.get_contact(*id).await?;
            match contact {
                Some(c) => {
                    if cli.json {
                        out::write_json(&c)?;
                    } else {
                        println!("ID: {}", c.user_id);
                        println!("Name: {} {}", c.first_name, c.last_name);
                        if let Some(u) = &c.username {
                            println!("Username: @{}", u);
                        }
                        if !c.phone.is_empty() {
                            println!("Phone: {}", c.phone);
                        }
                    }
                }
                None => {
                    anyhow::bail!("Contact {} not found", id);
                }
            }
        }
    }
    Ok(())
}
