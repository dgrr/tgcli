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

    /// Topic ID (for forum groups)
    #[arg(long)]
    pub topic: Option<i32>,

    /// Cancel typing indicator instead of sending
    #[arg(long, default_value_t = false)]
    pub cancel: bool,
}

pub async fn run(cli: &Cli, args: &TypingArgs) -> Result<()> {
    let app = App::new(cli).await?;

    if args.cancel {
        app.cancel_typing(args.chat, args.topic).await?;

        if cli.output.is_json() {
            let mut json = serde_json::json!({
                "typing": false,
                "chat": args.chat,
                "action": "cancelled"
            });
            if let Some(topic_id) = args.topic {
                json["topic"] = serde_json::json!(topic_id);
            }
            out::write_json(&json)?;
        } else {
            println!("Typing indicator cancelled.");
        }
    } else {
        app.set_typing(args.chat, args.topic).await?;

        if cli.output.is_json() {
            let mut json = serde_json::json!({
                "typing": true,
                "chat": args.chat,
                "action": "started"
            });
            if let Some(topic_id) = args.topic {
                json["topic"] = serde_json::json!(topic_id);
            }
            out::write_json(&json)?;
        } else if let Some(topic_id) = args.topic {
            println!("Typing indicator sent to topic {}.", topic_id);
        } else {
            println!("Typing indicator sent.");
        }
    }

    Ok(())
}
