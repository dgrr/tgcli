use crate::out;
use crate::store::Store;
use crate::Cli;
use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand, Debug, Clone)]
pub enum ContactsCommand {
    /// List all contacts
    List {
        /// Limit results
        #[arg(long)]
        limit: Option<i64>,
    },
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
        ContactsCommand::List { limit } => {
            let contacts = store.list_contacts(*limit).await?;

            if cli.output.is_json() {
                out::write_json(&contacts)?;
            } else if cli.output.is_markdown() {
                cli.output.write_titled(&contacts, "Contacts")?;
            } else {
                cli.output.write(&contacts)?;
            }
        }
        ContactsCommand::Search { query, limit } => {
            let contacts = store.search_contacts(query, *limit).await?;

            if cli.output.is_json() {
                out::write_json(&contacts)?;
            } else if cli.output.is_markdown() {
                cli.output.write_titled(&contacts, &format!("Contacts matching \"{}\"", query))?;
            } else {
                cli.output.write(&contacts)?;
            }
        }
        ContactsCommand::Show { id } => {
            let contact = store.get_contact(*id).await?;
            match contact {
                Some(c) => {
                    cli.output.write(&c)?;
                }
                None => {
                    anyhow::bail!(
                        "Contact {} not found. Run `tgcli sync` to refresh your contacts.",
                        id
                    );
                }
            }
        }
    }
    Ok(())
}
