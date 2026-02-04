use crate::app::App;
use crate::out;
use crate::Cli;
use anyhow::Result;
use clap::Args;

#[derive(Args, Debug, Clone)]
pub struct ReadArgs {
    /// Chat ID
    #[arg(long)]
    pub chat: i64,

    /// Message ID (mark up to this message as read)
    #[arg(long)]
    pub message: Option<i64>,
}

pub async fn run(cli: &Cli, args: &ReadArgs) -> Result<()> {
    let store_dir = cli.store_dir();

    // Try socket first
    if crate::app::socket::is_socket_available(&store_dir) {
        let resp = crate::app::socket::send_request(
            &store_dir,
            crate::app::socket::SocketRequest::MarkRead {
                chat: args.chat,
                message: args.message,
            },
        )
        .await?;

        if resp.ok {
            if cli.json {
                out::write_json(&serde_json::json!({ "marked_read": true }))?;
            } else {
                println!("Marked as read.");
            }
            return Ok(());
        }
    }

    // Fallback: direct connection
    let mut app = App::new(cli).await?;
    app.mark_read(args.chat).await?;

    if cli.json {
        out::write_json(&serde_json::json!({ "marked_read": true }))?;
    } else {
        println!("Marked as read.");
    }

    Ok(())
}
