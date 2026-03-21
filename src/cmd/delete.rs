use crate::app::SendApp;
use crate::out;
use crate::Cli;
use anyhow::Result;
use clap::Args;

#[derive(Args, Debug, Clone)]
pub struct DeleteArgs {
    /// Chat ID
    #[arg(long)]
    pub chat: i64,

    /// Message ID(s) to delete (repeatable)
    #[arg(long = "message", value_name = "MSG_ID")]
    pub messages: Vec<i64>,
}

pub async fn run(cli: &Cli, args: &DeleteArgs) -> Result<()> {
    if args.messages.is_empty() {
        anyhow::bail!("At least one --message is required");
    }

    // Use SendApp - lightweight, no store DB access
    let app = SendApp::new(cli).await?;

    let deleted = app.delete_messages(args.chat, &args.messages).await?;

    if cli.output.is_json() {
        out::write_json(&serde_json::json!({
            "deleted": true,
            "chat_id": args.chat,
            "message_ids": args.messages,
            "affected_count": deleted,
        }))?;
    } else {
        println!(
            "Deleted {} message(s) from chat {} (affected: {})",
            args.messages.len(),
            args.chat,
            deleted
        );
    }

    Ok(())
}
