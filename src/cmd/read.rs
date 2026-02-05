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

    /// Topic ID (for forum groups - marks a specific topic as read)
    #[arg(long)]
    pub topic: Option<i32>,

    /// Mark all topics as read (for forum groups)
    #[arg(long)]
    pub all_topics: bool,
}

pub async fn run(cli: &Cli, args: &ReadArgs) -> Result<()> {
    let store_dir = cli.store_dir();

    // Validate: --topic and --all-topics are mutually exclusive
    if args.topic.is_some() && args.all_topics {
        anyhow::bail!("Cannot use both --topic and --all-topics at the same time");
    }

    // Try socket first (but only for simple chat read, not topics)
    if args.topic.is_none() && !args.all_topics {
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
    }

    // Fallback: direct connection
    let mut app = App::new(cli).await?;

    if args.all_topics {
        // Mark all topics as read
        let count = app.mark_read_all_topics(args.chat).await?;

        if cli.json {
            out::write_json(&serde_json::json!({
                "marked_read": true,
                "topics_count": count
            }))?;
        } else {
            println!("Marked {} topics as read.", count);
        }
    } else if let Some(topic_id) = args.topic {
        // Mark a specific topic as read
        app.mark_read(args.chat, Some(topic_id)).await?;

        if cli.json {
            out::write_json(&serde_json::json!({
                "marked_read": true,
                "topic_id": topic_id
            }))?;
        } else {
            println!("Marked topic {} as read.", topic_id);
        }
    } else {
        // Mark the whole chat as read (or a single topic if --topic was given but not --all-topics)
        app.mark_read(args.chat, None).await?;

        if cli.json {
            out::write_json(&serde_json::json!({ "marked_read": true }))?;
        } else {
            println!("Marked as read.");
        }
    }

    Ok(())
}
