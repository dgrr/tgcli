pub mod auth;
pub mod chats;
pub mod clear;
pub mod completions;
pub mod contacts;
pub mod folders;
pub mod messages;
pub mod polls;
pub mod profile;
pub mod read;
pub mod send;
pub mod stickers;
pub mod sync;
pub mod topics;
pub mod typing;
pub mod users;
pub mod version;

use crate::Cli;
use clap::Subcommand;

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    /// Authenticate with Telegram
    Auth(auth::AuthArgs),
    /// Sync messages from Telegram
    Sync(sync::SyncArgs),
    /// Clear local database (keeps session)
    Clear(clear::ClearArgs),
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
    /// Create and vote in polls
    Polls {
        #[command(subcommand)]
        cmd: polls::PollsCommand,
    },
    /// List and show forum topics
    Topics {
        #[command(subcommand)]
        cmd: topics::TopicsCommand,
    },
    /// Manage chat folders (filters)
    Folders {
        #[command(subcommand)]
        cmd: folders::FoldersCommand,
    },
    /// Show user info, block and unblock users
    Users {
        #[command(subcommand)]
        cmd: users::UsersCommand,
    },
    /// Send typing indicator
    Typing(typing::TypingArgs),
    /// View and update your profile
    Profile {
        #[command(subcommand)]
        cmd: profile::ProfileCommand,
    },
    /// Show version info
    Version,
    /// Generate shell completions
    Completions {
        /// Shell type to generate completions for
        #[arg(value_enum)]
        shell: completions::ShellType,
    },
}

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    match &cli.command {
        Command::Auth(args) => auth::run(&cli, args).await,
        Command::Sync(args) => sync::run(&cli, args).await,
        Command::Clear(args) => clear::run(&cli, args).await,
        Command::Chats { cmd } => chats::run(&cli, cmd).await,
        Command::Messages { cmd } => messages::run(&cli, cmd).await,
        Command::Send(args) => send::run(&cli, args).await,
        Command::Contacts { cmd } => contacts::run(&cli, cmd).await,
        Command::Read(args) => read::run(&cli, args).await,
        Command::Stickers { cmd } => stickers::run(&cli, cmd).await,
        Command::Polls { cmd } => polls::run(&cli, cmd).await,
        Command::Topics { cmd } => topics::run(&cli, cmd).await,
        Command::Folders { cmd } => folders::run(&cli, cmd).await,
        Command::Users { cmd } => users::run(&cli, cmd).await,
        Command::Typing(args) => typing::run(&cli, args).await,
        Command::Profile { cmd } => profile::run(&cli, cmd).await,
        Command::Version => {
            version::run(&cli);
            Ok(())
        }
        Command::Completions { shell } => completions::run(shell),
    }
}
