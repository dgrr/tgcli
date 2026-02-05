pub mod auth;
pub mod chats;
pub mod contacts;
pub mod messages;
pub mod read;
pub mod send;
pub mod stickers;
pub mod sync;
pub mod version;

use crate::Cli;
use clap::Subcommand;

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    /// Authenticate with Telegram
    Auth(auth::AuthArgs),
    /// Sync messages from Telegram
    Sync(sync::SyncArgs),
    /// List and show chats
    Chats {
        #[command(subcommand)]
        cmd: chats::ChatsCommand,
    },
    /// List, search, and show messages
    Messages {
        #[command(subcommand)]
        cmd: messages::MessagesCommand,
    },
    /// Send a message
    Send(send::SendArgs),
    /// Search and show contacts
    Contacts {
        #[command(subcommand)]
        cmd: contacts::ContactsCommand,
    },
    /// Mark messages as read
    Read(read::ReadArgs),
    /// List, show, and search stickers
    Stickers {
        #[command(subcommand)]
        cmd: stickers::StickersCommand,
    },
    /// Show version info
    Version,
}

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    match &cli.command {
        Command::Auth(args) => auth::run(&cli, args).await,
        Command::Sync(args) => sync::run(&cli, args).await,
        Command::Chats { cmd } => chats::run(&cli, cmd).await,
        Command::Messages { cmd } => messages::run(&cli, cmd).await,
        Command::Send(args) => send::run(&cli, args).await,
        Command::Contacts { cmd } => contacts::run(&cli, cmd).await,
        Command::Read(args) => read::run(&cli, args).await,
        Command::Stickers { cmd } => stickers::run(&cli, cmd).await,
        Command::Version => {
            version::run(&cli);
            Ok(())
        }
    }
}
