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

    /// Message text
    #[arg(long)]
    pub message: String,
}

pub async fn run(cli: &Cli, args: &SendArgs) -> Result<()> {
    let store_dir = cli.store_dir();

    // Try socket first (sync process may be running)
    if crate::app::socket::is_socket_available(&store_dir) {
        let resp = crate::app::socket::send_request(
            &store_dir,
            crate::app::socket::SocketRequest::SendText {
                to: args.to,
                message: args.message.clone(),
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
    let msg_id = app.send_text(args.to, &args.message).await?;

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
