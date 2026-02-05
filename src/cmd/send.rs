use crate::app::App;
use crate::out;
use crate::Cli;
use anyhow::Result;
use clap::Args;

#[derive(Args, Debug, Clone)]
pub struct SendArgs {
    /// Recipient chat ID
    #[arg(long)]
    pub to: i64,

    /// Message text (required unless --sticker is provided)
    #[arg(long, required_unless_present = "sticker")]
    pub message: Option<String>,

    /// Sticker file_id (from `tgcli stickers show --pack <pack>`)
    #[arg(long, conflicts_with = "message")]
    pub sticker: Option<String>,
}

pub async fn run(cli: &Cli, args: &SendArgs) -> Result<()> {
    let store_dir = cli.store_dir();

    // Handle sticker sending
    if let Some(ref sticker_id) = args.sticker {
        // Stickers always use direct connection (no socket support yet)
        let mut app = App::new(cli).await?;
        let msg_id = app.send_sticker(args.to, sticker_id).await?;

        if cli.json {
            out::write_json(&serde_json::json!({
                "sent": true,
                "to": args.to,
                "id": msg_id,
                "type": "sticker",
            }))?;
        } else {
            println!("Sticker sent to {}", args.to);
        }
        return Ok(());
    }

    // Handle text message
    let message = args
        .message
        .as_ref()
        .expect("message required when no sticker");

    // Try socket first (sync process may be running)
    if crate::app::socket::is_socket_available(&store_dir) {
        let resp = crate::app::socket::send_request(
            &store_dir,
            crate::app::socket::SocketRequest::SendText {
                to: args.to,
                message: message.clone(),
            },
        )
        .await?;

        if resp.ok {
            if cli.json {
                out::write_json(&serde_json::json!({
                    "sent": true,
                    "to": args.to,
                    "id": resp.id,
                }))?;
            } else {
                println!("Sent to {} (via socket)", args.to);
            }
            return Ok(());
        } else {
            anyhow::bail!("Socket send failed: {}", resp.error.unwrap_or_default());
        }
    }

    // Fallback: direct connection
    let mut app = App::new(cli).await?;
    let msg_id = app.send_text(args.to, message).await?;

    if cli.json {
        out::write_json(&serde_json::json!({
            "sent": true,
            "to": args.to,
            "id": msg_id,
        }))?;
    } else {
        println!("Sent to {}", args.to);
    }
    Ok(())
}
