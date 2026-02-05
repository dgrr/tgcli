use crate::app::App;
use crate::out;
use crate::Cli;
use anyhow::Result;
use clap::Args;

#[derive(Args, Debug, Clone)]
pub struct TypingArgs {
    /// Chat ID
    #[arg(long)]
    pub chat: i64,

    /// Cancel typing indicator instead of sending
    #[arg(long, default_value_t = false)]
    pub cancel: bool,
}

pub async fn run(cli: &Cli, args: &TypingArgs) -> Result<()> {
    let app = App::new(cli).await?;

    if args.cancel {
        app.cancel_typing(args.chat).await?;

        if cli.json {
            out::write_json(&serde_json::json!({
                "typing": false,
                "chat": args.chat,
                "action": "cancelled"
            }))?;
        } else {
            println!("Typing indicator cancelled.");
        }
    } else {
        app.set_typing(args.chat).await?;

        if cli.json {
            out::write_json(&serde_json::json!({
                "typing": true,
                "chat": args.chat,
                "action": "started"
            }))?;
        } else {
            println!("Typing indicator sent.");
        }
    }

    Ok(())
}
