use crate::app::App;
use crate::out;
use crate::Cli;
use anyhow::Result;
use chrono::{DateTime, Utc};
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

    /// Send a file as document (any file type, preserves original filename)
    #[arg(long, conflicts_with_all = ["sticker", "photo", "video", "voice"])]
    pub file: Option<PathBuf>,

    /// Send an audio file as voice message (inline playback in Telegram)
    #[arg(long, conflicts_with_all = ["sticker", "photo", "video", "file"])]
    pub voice: Option<PathBuf>,

    /// Forum topic ID (for sending to a specific topic in a forum/supergroup)
    #[arg(long)]
    pub topic: Option<i32>,

    /// Reply to a specific message ID
    #[arg(long)]
    pub reply_to: Option<i32>,

    /// Caption for media (photo, video, file, voice)
    #[arg(long)]
    pub caption: Option<String>,

    /// Schedule message for a specific time (RFC3339 format, e.g. "2026-02-06T10:00:00Z")
    #[arg(long, conflicts_with = "schedule_in")]
    pub schedule: Option<String>,

    /// Schedule message to be sent in N seconds from now
    #[arg(long, conflicts_with = "schedule")]
    pub schedule_in: Option<i64>,
}

/// Parse schedule arguments and return the scheduled DateTime if provided
fn parse_schedule(
    schedule: &Option<String>,
    schedule_in: &Option<i64>,
) -> Result<Option<DateTime<Utc>>> {
    if let Some(ref schedule_str) = schedule {
        // Try parsing as RFC3339 with timezone
        if let Ok(dt) = DateTime::parse_from_rfc3339(schedule_str) {
            return Ok(Some(dt.with_timezone(&Utc)));
        }
        // Try parsing as local datetime without timezone (assume UTC)
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(schedule_str, "%Y-%m-%dT%H:%M:%S")
        {
            return Ok(Some(naive.and_utc()));
        }
        anyhow::bail!(
            "Invalid schedule format '{}'. Use RFC3339 format (e.g. '2026-02-06T10:00:00Z' or '2026-02-06T10:00:00')",
            schedule_str
        );
    }
    if let Some(seconds) = schedule_in {
        if *seconds <= 0 {
            anyhow::bail!("--schedule-in must be a positive number of seconds");
        }
        let scheduled_time = Utc::now() + chrono::Duration::seconds(*seconds);
        return Ok(Some(scheduled_time));
    }
    Ok(None)
}

pub async fn run(cli: &Cli, args: &SendArgs) -> Result<()> {
    let store_dir = cli.store_dir();

    // Parse schedule options
    let schedule_time = parse_schedule(&args.schedule, &args.schedule_in)?;

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

    // Handle video sending
    if let Some(ref video_path) = args.video {
        if args.topic.is_some() {
            anyhow::bail!("--topic is not supported with --video yet");
        }
        let mut app = App::new(cli).await?;
        let caption = args.caption.as_deref().unwrap_or("");
        let msg_id = app.send_video(args.to, video_path, caption).await?;

        if cli.json {
            out::write_json(&serde_json::json!({
                "sent": true,
                "to": args.to,
                "id": msg_id,
                "type": "video",
            }))?;
        } else {
            println!("Video sent to {}", args.to);
        }
        return Ok(());
    }

    // Handle file (document) sending
    if let Some(ref file_path) = args.file {
        if args.topic.is_some() {
            anyhow::bail!("--topic is not supported with --file yet");
        }
        let mut app = App::new(cli).await?;
        let caption = args.caption.as_deref().unwrap_or("");
        let msg_id = app.send_file(args.to, file_path, caption).await?;

        if cli.json {
            out::write_json(&serde_json::json!({
                "sent": true,
                "to": args.to,
                "id": msg_id,
                "type": "document",
            }))?;
        } else {
            println!("File sent to {}", args.to);
        }
        return Ok(());
    }

    // Handle voice message sending
    if let Some(ref voice_path) = args.voice {
        if args.topic.is_some() {
            anyhow::bail!("--topic is not supported with --voice yet");
        }
        let mut app = App::new(cli).await?;
        let caption = args.caption.as_deref().unwrap_or("");
        let msg_id = app.send_voice(args.to, voice_path, caption).await?;

        if cli.json {
            out::write_json(&serde_json::json!({
                "sent": true,
                "to": args.to,
                "id": msg_id,
                "type": "voice",
            }))?;
        } else {
            println!("Voice message sent to {}", args.to);
        }
        return Ok(());
    }

    // Handle text message
    let message = args
        .message
        .as_ref()
        .expect("message required when no sticker");

    // Direct connection
    let mut app = App::new(cli).await?;

    let msg_id = if let Some(topic_id) = args.topic {
        if schedule_time.is_some() {
            anyhow::bail!("--schedule/--schedule-in is not supported with --topic yet");
        }
        app.send_text_to_topic(args.to, topic_id, message).await?
    } else if let Some(reply_to_id) = args.reply_to {
        if schedule_time.is_some() {
            anyhow::bail!("--schedule/--schedule-in is not supported with --reply-to yet");
        }
        app.send_text_reply(args.to, message, reply_to_id).await?
    } else if let Some(schedule_dt) = schedule_time {
        app.send_text_scheduled(args.to, message, schedule_dt)
            .await?
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
        if let Some(ref schedule_str) = args.schedule {
            json["scheduled"] = serde_json::json!(schedule_str);
        }
        if let Some(schedule_in_secs) = args.schedule_in {
            json["scheduled_in"] = serde_json::json!(schedule_in_secs);
        }
        out::write_json(&json)?;
    } else if let Some(topic_id) = args.topic {
        println!("Sent to {} topic {}", args.to, topic_id);
    } else if let Some(reply_to_id) = args.reply_to {
        println!("Sent reply to {} (replying to {})", args.to, reply_to_id);
    } else if schedule_time.is_some() {
        println!("Scheduled message to {}", args.to);
    } else {
        println!("Sent to {}", args.to);
    }
    Ok(())
}
