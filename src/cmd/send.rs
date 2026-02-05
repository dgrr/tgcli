use crate::app::App;
use crate::out;
use crate::Cli;
use anyhow::Result;
use clap::Args;
use std::path::PathBuf;

#[derive(Args, Debug, Clone)]
pub struct SendArgs {
    /// Recipient chat ID
    #[arg(long)]
    pub to: i64,

    /// Message text (required unless --sticker or media is provided)
    #[arg(long, required_unless_present_any = ["sticker", "photo", "video", "file", "voice"])]
    pub message: Option<String>,

    /// Sticker file_id (from `tgcli stickers show --pack <pack>`)
    #[arg(long, conflicts_with_all = ["message", "photo", "video", "file", "voice"])]
    pub sticker: Option<String>,

    /// Send a photo (path to image file)
    #[arg(long, conflicts_with_all = ["sticker", "video", "file", "voice"])]
    pub photo: Option<PathBuf>,

    /// Send a video (path to video file)
    #[arg(long, conflicts_with_all = ["sticker", "photo", "file", "voice"])]
    pub video: Option<PathBuf>,

    /// Forum topic ID (for sending to a specific topic in a forum/supergroup)
    #[arg(long)]
    pub topic: Option<i32>,

    /// Reply to a specific message ID
    #[arg(long)]
    pub reply_to: Option<i32>,

    /// Caption for media (photo, video, file, voice)
    #[arg(long)]
    pub caption: Option<String>,
}

pub async fn run(cli: &Cli, args: &SendArgs) -> Result<()> {
    let store_dir = cli.store_dir();

    // Handle sticker sending
    if let Some(ref sticker_id) = args.sticker {
        if args.topic.is_some() {
            anyhow::bail!("--topic is not supported with --sticker yet");
        }
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

    // Handle photo sending
    if let Some(ref photo_path) = args.photo {
        if args.topic.is_some() {
            anyhow::bail!("--topic is not supported with --photo yet");
        }
        let mut app = App::new(cli).await?;
        let caption = args.caption.as_deref().unwrap_or("");
        let msg_id = app.send_photo(args.to, photo_path, caption).await?;

        if cli.json {
            out::write_json(&serde_json::json!({
                "sent": true,
                "to": args.to,
                "id": msg_id,
                "type": "photo",
            }))?;
        } else {
            println!("Photo sent to {}", args.to);
        }
        return Ok(());
    }

    // Handle text message
    let message = args
        .message
        .as_ref()
        .expect("message required when no sticker");

    // Try socket first (sync process may be running) - but not for topic/reply messages yet
    if args.topic.is_none()
        && args.reply_to.is_none()
        && crate::app::socket::is_socket_available(&store_dir)
    {
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

    // Direct connection (required for topic/reply messages)
    let mut app = App::new(cli).await?;

    let msg_id = if let Some(topic_id) = args.topic {
        app.send_text_to_topic(args.to, topic_id, message).await?
    } else if let Some(reply_to_id) = args.reply_to {
        app.send_text_reply(args.to, message, reply_to_id).await?
    } else {
        app.send_text(args.to, message).await?
    };

    if cli.json {
        let mut json = serde_json::json!({
            "sent": true,
            "to": args.to,
            "id": msg_id,
        });
        if let Some(topic_id) = args.topic {
            json["topic"] = serde_json::json!(topic_id);
        }
        if let Some(reply_to_id) = args.reply_to {
            json["reply_to"] = serde_json::json!(reply_to_id);
        }
        out::write_json(&json)?;
    } else if let Some(topic_id) = args.topic {
        println!("Sent to {} topic {}", args.to, topic_id);
    } else if let Some(reply_to_id) = args.reply_to {
        println!("Sent reply to {} (replying to {})", args.to, reply_to_id);
    } else {
        println!("Sent to {}", args.to);
    }
    Ok(())
}
