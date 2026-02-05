use crate::app::App;
use crate::out;
use crate::Cli;
use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Subcommand, Debug, Clone)]
pub enum PollsCommand {
    /// Create a poll
    Create(CreateArgs),
    /// Vote in a poll
    Vote(VoteArgs),
}

#[derive(Args, Debug, Clone)]
pub struct CreateArgs {
    /// Chat ID to send the poll to
    #[arg(long)]
    pub chat: i64,

    /// Poll question
    #[arg(long)]
    pub question: String,

    /// Poll options (2-10 options, use multiple --option flags)
    #[arg(long = "option", required = true, num_args = 1)]
    pub options: Vec<String>,

    /// Allow multiple answers
    #[arg(long, default_value_t = false)]
    pub multiple: bool,

    /// Make poll anonymous (default: true)
    #[arg(long, default_value_t = true)]
    pub anonymous: bool,
}

#[derive(Args, Debug, Clone)]
pub struct VoteArgs {
    /// Chat ID where the poll is
    #[arg(long)]
    pub chat: i64,

    /// Message ID of the poll
    #[arg(long)]
    pub message: i64,

    /// Option index to vote for (0-based, can specify multiple for multi-choice polls)
    #[arg(long = "option", required = true, num_args = 1)]
    pub options: Vec<usize>,
}

pub async fn run(cli: &Cli, cmd: &PollsCommand) -> Result<()> {
    match cmd {
        PollsCommand::Create(args) => create_poll(cli, args).await,
        PollsCommand::Vote(args) => vote_poll(cli, args).await,
    }
}

async fn create_poll(cli: &Cli, args: &CreateArgs) -> Result<()> {
    // Validate options count
    if args.options.len() < 2 {
        anyhow::bail!("Poll must have at least 2 options");
    }
    if args.options.len() > 10 {
        anyhow::bail!("Poll can have at most 10 options");
    }

    let mut app = App::new(cli).await?;
    let msg_id = app
        .send_poll(
            args.chat,
            &args.question,
            &args.options,
            args.multiple,
            !args.anonymous, // public_voters is inverse of anonymous
        )
        .await?;

    if cli.json {
        out::write_json(&serde_json::json!({
            "sent": true,
            "to": args.chat,
            "id": msg_id,
            "type": "poll",
            "question": args.question,
            "options": args.options,
            "multiple_choice": args.multiple,
        }))?;
    } else {
        println!(
            "Poll created in chat {} (message ID: {})",
            args.chat, msg_id
        );
    }
    Ok(())
}

async fn vote_poll(cli: &Cli, args: &VoteArgs) -> Result<()> {
    let app = App::new(cli).await?;
    app.vote_poll(args.chat, args.message, &args.options)
        .await?;

    if cli.json {
        out::write_json(&serde_json::json!({
            "voted": true,
            "chat": args.chat,
            "message": args.message,
            "options": args.options,
        }))?;
    } else {
        println!(
            "Voted in poll (chat: {}, message: {})",
            args.chat, args.message
        );
    }
    Ok(())
}
