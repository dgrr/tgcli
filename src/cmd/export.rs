use crate::app::App;
use crate::store::Store;
use crate::Cli;
use anyhow::Result;
use chrono::{DateTime, Local, NaiveTime, TimeZone, Utc};
use clap::{Args, ValueEnum};
use std::fs::File;
use std::io::{BufWriter, Write};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ExportFormat {
    Json,
    Html,
}

#[derive(Args, Debug, Clone)]
pub struct ExportArgs {
    /// Chat ID to export
    #[arg(long)]
    pub chat: i64,

    /// Output format
    #[arg(long, value_enum, default_value = "json")]
    pub format: ExportFormat,

    /// Output file path (defaults to stdout for JSON, chat_<id>.html for HTML)
    #[arg(long, short = 'o')]
    pub output: Option<String>,

    /// Only messages after this date (YYYY-MM-DD or RFC3339)
    #[arg(long)]
    pub since: Option<String>,

    /// Only messages before this date (YYYY-MM-DD or RFC3339)
    #[arg(long)]
    pub until: Option<String>,

    /// Fetch messages from Telegram API instead of local database
    #[arg(long)]
    pub fetch: bool,

    /// Maximum number of messages to export (default: all)
    #[arg(long)]
    pub limit: Option<usize>,
}

pub async fn run(cli: &Cli, args: &ExportArgs) -> Result<()> {
    let store = Store::open(&cli.store_dir()).await?;

    // Get chat info
    let chat = store.get_chat(args.chat).await?;
    let chat_name = chat
        .as_ref()
        .map(|c| c.name.clone())
        .unwrap_or_else(|| format!("Chat {}", args.chat));

    // Parse date filters
    let since = args.since.as_deref().map(parse_date).transpose()?;
    let until = args.until.as_deref().map(parse_date).transpose()?;

    // Collect messages
    let messages = if args.fetch {
        // Fetch from Telegram API
        let app = App::new(cli).await?;
        fetch_messages_from_api(&app, args.chat, since, until, args.limit).await?
    } else {
        // Use local database
        fetch_messages_from_store(&store, args.chat, since, until, args.limit).await?
    };

    eprintln!(
        "Exporting {} messages from \"{}\"...",
        messages.len(),
        chat_name
    );

    // Export based on format
    match args.format {
        ExportFormat::Json => {
            export_json(&messages, args.output.as_deref())?;
        }
        ExportFormat::Html => {
            let output_path = args
                .output
                .clone()
                .unwrap_or_else(|| format!("chat_{}.html", args.chat));
            export_html(&messages, &output_path, &chat_name, args.chat)?;
            eprintln!("Exported to: {}", output_path);
        }
    }

    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
struct ExportMessage {
    id: i64,
    chat_id: i64,
    sender_id: i64,
    from_me: bool,
    ts: String,
    edit_ts: Option<String>,
    text: String,
    media_type: Option<String>,
    reply_to_id: Option<i64>,
    topic_id: Option<i32>,
}

async fn fetch_messages_from_store(
    store: &Store,
    chat_id: i64,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
    limit: Option<usize>,
) -> Result<Vec<ExportMessage>> {
    use crate::store::ListMessagesParams;

    // Fetch all messages (use a large limit)
    let db_limit = limit.unwrap_or(1_000_000) as i64;

    let msgs = store
        .list_messages(ListMessagesParams {
            chat_id: Some(chat_id),
            topic_id: None,
            limit: db_limit,
            after: since,
            before: until,
            ignore_chats: vec![],
            ignore_channels: false,
        })
        .await?;

    Ok(msgs
        .into_iter()
        .map(|m| ExportMessage {
            id: m.id,
            chat_id: m.chat_id,
            sender_id: m.sender_id,
            from_me: m.from_me,
            ts: m.ts.to_rfc3339(),
            edit_ts: m.edit_ts.map(|t| t.to_rfc3339()),
            text: m.text,
            media_type: m.media_type,
            reply_to_id: m.reply_to_id,
            topic_id: m.topic_id,
        })
        .collect())
}

async fn fetch_messages_from_api(
    app: &App,
    chat_id: i64,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
    limit: Option<usize>,
) -> Result<Vec<ExportMessage>> {
    use grammers_session::defs::PeerRef;

    let peer_ref = resolve_peer_ref(app, chat_id).await?;

    let mut message_iter = app.tg.client.iter_messages(peer_ref);
    let mut messages = Vec::new();
    let max_count = limit.unwrap_or(usize::MAX);

    while let Some(msg) = message_iter.next().await? {
        let msg_ts = msg.date();

        // Apply date filters
        if let Some(ref since_ts) = since {
            if msg_ts < *since_ts {
                // Messages are in reverse chronological order, so we can stop
                break;
            }
        }
        if let Some(ref until_ts) = until {
            if msg_ts > *until_ts {
                continue;
            }
        }

        let sender_id = msg.sender().map(|s| s.id().bare_id()).unwrap_or(0);
        let from_me = msg.outgoing();

        messages.push(ExportMessage {
            id: msg.id() as i64,
            chat_id,
            sender_id,
            from_me,
            ts: msg_ts.to_rfc3339(),
            edit_ts: msg.edit_date().map(|t| t.to_rfc3339()),
            text: msg.text().to_string(),
            media_type: msg.media().map(|_| "media".to_string()),
            reply_to_id: msg.reply_to_message_id().map(|id| id as i64),
            topic_id: None, // TODO: extract topic_id if needed
        });

        if messages.len() >= max_count {
            break;
        }
    }

    // Reverse to chronological order
    messages.reverse();
    Ok(messages)
}

fn export_json(messages: &[ExportMessage], output: Option<&str>) -> Result<()> {
    if let Some(path) = output {
        // Write to file as JSONL
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);

        for msg in messages {
            serde_json::to_writer(&mut writer, msg)?;
            writeln!(writer)?;
        }

        writer.flush()?;
        eprintln!("Exported to: {}", path);
    } else {
        // Write to stdout as JSONL
        for msg in messages {
            println!("{}", serde_json::to_string(msg)?);
        }
    }
    Ok(())
}

fn export_html(
    messages: &[ExportMessage],
    output_path: &str,
    chat_name: &str,
    chat_id: i64,
) -> Result<()> {
    let file = File::create(output_path)?;
    let mut writer = BufWriter::new(file);

    // Write HTML header
    writeln!(
        writer,
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Chat Export: {}</title>
    <style>
        * {{
            box-sizing: border-box;
        }}
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
            max-width: 800px;
            margin: 0 auto;
            padding: 20px;
            background: #f5f5f5;
            color: #333;
        }}
        h1 {{
            text-align: center;
            color: #2196F3;
            margin-bottom: 10px;
        }}
        .meta {{
            text-align: center;
            color: #666;
            margin-bottom: 30px;
            font-size: 14px;
        }}
        .messages {{
            display: flex;
            flex-direction: column;
            gap: 10px;
        }}
        .message {{
            padding: 12px 16px;
            border-radius: 12px;
            max-width: 80%;
            word-wrap: break-word;
        }}
        .message.outgoing {{
            background: #dcf8c6;
            align-self: flex-end;
            margin-left: 20%;
        }}
        .message.incoming {{
            background: white;
            align-self: flex-start;
            margin-right: 20%;
            box-shadow: 0 1px 2px rgba(0,0,0,0.1);
        }}
        .message-header {{
            display: flex;
            justify-content: space-between;
            font-size: 12px;
            color: #666;
            margin-bottom: 6px;
        }}
        .sender {{
            font-weight: 600;
            color: #2196F3;
        }}
        .time {{
            color: #999;
        }}
        .text {{
            white-space: pre-wrap;
            line-height: 1.4;
        }}
        .media-badge {{
            display: inline-block;
            background: #e3f2fd;
            color: #1976d2;
            padding: 2px 8px;
            border-radius: 4px;
            font-size: 12px;
            margin-top: 6px;
        }}
        .reply-indicator {{
            font-size: 12px;
            color: #666;
            border-left: 2px solid #2196F3;
            padding-left: 8px;
            margin-bottom: 6px;
        }}
        .date-separator {{
            text-align: center;
            color: #666;
            font-size: 13px;
            margin: 20px 0;
            position: relative;
        }}
        .date-separator span {{
            background: #f5f5f5;
            padding: 0 16px;
        }}
        .date-separator::before {{
            content: '';
            position: absolute;
            left: 0;
            right: 0;
            top: 50%;
            height: 1px;
            background: #ddd;
            z-index: -1;
        }}
    </style>
</head>
<body>
    <h1>{}</h1>
    <div class="meta">
        Chat ID: {} | {} messages | Exported: {}
    </div>
    <div class="messages">"#,
        html_escape(chat_name),
        html_escape(chat_name),
        chat_id,
        messages.len(),
        Local::now().format("%Y-%m-%d %H:%M:%S")
    )?;

    // Track current date for date separators
    let mut current_date: Option<String> = None;

    for msg in messages {
        let ts = DateTime::parse_from_rfc3339(&msg.ts)
            .ok()
            .map(|dt| dt.with_timezone(&Local));

        let date_str = ts
            .as_ref()
            .map(|t| t.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        let time_str = ts
            .as_ref()
            .map(|t| t.format("%H:%M").to_string())
            .unwrap_or_default();

        // Insert date separator if date changed
        if current_date.as_ref() != Some(&date_str) {
            current_date = Some(date_str.clone());
            writeln!(
                writer,
                r#"        <div class="date-separator"><span>{}</span></div>"#,
                date_str
            )?;
        }

        let class = if msg.from_me { "outgoing" } else { "incoming" };
        let sender = if msg.from_me {
            "You".to_string()
        } else {
            format!("User {}", msg.sender_id)
        };

        writeln!(writer, r#"        <div class="message {}">"#, class)?;

        // Reply indicator
        if let Some(reply_id) = msg.reply_to_id {
            writeln!(
                writer,
                r#"            <div class="reply-indicator">Reply to message #{}</div>"#,
                reply_id
            )?;
        }

        writeln!(
            writer,
            r#"            <div class="message-header">
                <span class="sender">{}</span>
                <span class="time">{}</span>
            </div>"#,
            html_escape(&sender),
            time_str
        )?;

        if !msg.text.is_empty() {
            writeln!(
                writer,
                r#"            <div class="text">{}</div>"#,
                html_escape(&msg.text)
            )?;
        }

        if let Some(ref media_type) = msg.media_type {
            writeln!(
                writer,
                r#"            <span class="media-badge">📎 {}</span>"#,
                html_escape(media_type)
            )?;
        }

        writeln!(writer, r#"        </div>"#)?;
    }

    // Write HTML footer
    writeln!(
        writer,
        r#"    </div>
</body>
</html>"#
    )?;

    writer.flush()?;
    Ok(())
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn parse_date(s: &str) -> Result<DateTime<Utc>> {
    use chrono::Duration;

    let s_lower = s.to_lowercase();

    // Handle relative time expressions
    if s_lower == "today" {
        let today = Local::now().date_naive();
        return Ok(today.and_time(NaiveTime::MIN).and_utc());
    }

    if s_lower == "yesterday" {
        let yesterday = Local::now().date_naive() - Duration::days(1);
        return Ok(yesterday.and_time(NaiveTime::MIN).and_utc());
    }

    // Try RFC3339 first
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }

    // Try YYYY-MM-DD
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let dt = d.and_hms_opt(0, 0, 0).unwrap().and_utc();
        return Ok(dt);
    }

    // Try YYYY-MM-DD HH:MM:SS
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Ok(Local.from_local_datetime(&dt).unwrap().with_timezone(&Utc));
    }

    anyhow::bail!(
        "Invalid date format: '{}'. Use: YYYY-MM-DD, RFC3339, 'today', or 'yesterday'",
        s
    )
}

async fn resolve_peer_ref(app: &App, chat_id: i64) -> Result<grammers_session::defs::PeerRef> {
    use grammers_session::defs::PeerRef;

    let mut dialogs = app.tg.client.iter_dialogs();
    while let Some(dialog) = dialogs.next().await? {
        let peer = dialog.peer();
        if peer.id().bare_id() == chat_id {
            return Ok(PeerRef::from(peer));
        }
    }
    anyhow::bail!(
        "Chat {} not found. Run `tgcli sync` to refresh your chat list.",
        chat_id
    )
}
