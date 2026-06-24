//! Inline keyboard button support: list a message's inline buttons and "click"
//! a callback button (via `messages.getBotCallbackAnswer`), optionally waiting
//! for and downloading the bot's follow-up message.
//!
//! grammers exposes received buttons through `Message::reply_markup()` but has
//! no high-level "click"; the click is done with the raw API. This module wires
//! that up so the CLI can drive bots that answer with inline keyboards (e.g.
//! search bots that deliver a file when you tap a result).

use crate::app::App;
use anyhow::{Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use grammers_client::types::Message;
use grammers_session::defs::PeerRef;
use grammers_tl_types as tl;
use serde::Serialize;
use std::time::{Duration, Instant};

/// A single inline keyboard button, flattened across rows.
#[derive(Debug, Clone, Serialize)]
pub struct ButtonInfo {
    /// 0-based index across all rows (use with `messages click --button`).
    pub index: usize,
    pub row: usize,
    pub col: usize,
    pub text: String,
    /// "callback" | "url" | "webview" | "switch_inline" | "other"
    pub kind: String,
    /// Callback payload as URL-safe base64 (only for callback buttons).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    /// Callback payload decoded as UTF-8, when printable (informational).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_text: Option<String>,
    /// Target URL / inline query, for url/webview/switch_inline buttons.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// A compact, live view of a message (fetched straight from Telegram).
#[derive(Debug, Clone, Serialize)]
pub struct LatestMessageInfo {
    pub id: i64,
    pub buttons: bool,
    pub media: bool,
    pub text: String,
}

/// A message that arrived after a click (the bot's follow-up).
#[derive(Debug, Clone, Serialize)]
pub struct NewMessageInfo {
    pub id: i64,
    pub has_media: bool,
    pub text: String,
}

/// Result of clicking a button.
#[derive(Debug, Clone, Serialize)]
pub struct ClickOutcome {
    /// Bot's toast/alert text, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// URL the answer points to, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Whether the answer was shown as an alert (vs a toast).
    pub alert: bool,
    /// New messages observed after the click (within the wait window).
    pub new_messages: Vec<NewMessageInfo>,
    /// Local paths of files downloaded from the new messages.
    pub downloaded: Vec<String>,
}

/// Flatten an inline keyboard markup into a list of buttons.
fn extract_buttons(markup: &tl::enums::ReplyMarkup) -> Vec<ButtonInfo> {
    let mut out = Vec::new();
    let tl::enums::ReplyMarkup::ReplyInlineMarkup(inline) = markup else {
        return out;
    };
    let mut index = 0usize;
    for (ri, row) in inline.rows.iter().enumerate() {
        let tl::enums::KeyboardButtonRow::Row(row) = row;
        for (ci, btn) in row.buttons.iter().enumerate() {
            let info = match btn {
                tl::enums::KeyboardButton::Callback(b) => ButtonInfo {
                    index,
                    row: ri,
                    col: ci,
                    text: b.text.clone(),
                    kind: "callback".into(),
                    data: Some(URL_SAFE_NO_PAD.encode(&b.data)),
                    data_text: std::str::from_utf8(&b.data)
                        .ok()
                        .filter(|s| s.chars().all(|c| !c.is_control()))
                        .map(|s| s.to_string()),
                    url: None,
                },
                tl::enums::KeyboardButton::Url(b) => ButtonInfo {
                    index,
                    row: ri,
                    col: ci,
                    text: b.text.clone(),
                    kind: "url".into(),
                    data: None,
                    data_text: None,
                    url: Some(b.url.clone()),
                },
                tl::enums::KeyboardButton::WebView(b) => ButtonInfo {
                    index,
                    row: ri,
                    col: ci,
                    text: b.text.clone(),
                    kind: "webview".into(),
                    data: None,
                    data_text: None,
                    url: Some(b.url.clone()),
                },
                tl::enums::KeyboardButton::SwitchInline(b) => ButtonInfo {
                    index,
                    row: ri,
                    col: ci,
                    text: b.text.clone(),
                    kind: "switch_inline".into(),
                    data: None,
                    data_text: Some(b.query.clone()),
                    url: None,
                },
                other => ButtonInfo {
                    index,
                    row: ri,
                    col: ci,
                    text: button_text(other),
                    kind: "other".into(),
                    data: None,
                    data_text: None,
                    url: None,
                },
            };
            out.push(info);
            index += 1;
        }
    }
    out
}

/// Best-effort label for button variants we don't specifically handle.
fn button_text(btn: &tl::enums::KeyboardButton) -> String {
    use tl::enums::KeyboardButton as K;
    match btn {
        K::Button(b) => b.text.clone(),
        K::RequestPhone(b) => b.text.clone(),
        K::RequestGeoLocation(b) => b.text.clone(),
        K::Game(b) => b.text.clone(),
        K::Buy(b) => b.text.clone(),
        K::UrlAuth(b) => b.text.clone(),
        K::RequestPoll(b) => b.text.clone(),
        K::UserProfile(b) => b.text.clone(),
        K::SimpleWebView(b) => b.text.clone(),
        K::RequestPeer(b) => b.text.clone(),
        K::Copy(b) => b.text.clone(),
        _ => String::new(),
    }
}

impl App {
    /// Id of the newest message currently in the chat (0 if none).
    async fn top_message_id(&self, peer_ref: PeerRef) -> Result<i32> {
        let mut it = self.tg.client.iter_messages(peer_ref).limit(1);
        Ok(it.next().await?.map(|m| m.id()).unwrap_or(0))
    }

    /// Fetch the latest messages for a chat directly from Telegram, bypassing
    /// the local database (so brand-new bot replies are visible without a sync).
    /// Returned newest-first.
    pub async fn latest_messages(
        &self,
        chat_id: i64,
        limit: usize,
    ) -> Result<Vec<LatestMessageInfo>> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;
        let mut it = self.tg.client.iter_messages(peer_ref).limit(limit);
        let mut out = Vec::new();
        while let Some(m) = it.next().await? {
            out.push(LatestMessageInfo {
                id: m.id() as i64,
                buttons: m.reply_markup().is_some(),
                media: m.media().is_some(),
                text: m.text().chars().take(4000).collect(),
            });
        }
        Ok(out)
    }

    /// List the inline keyboard buttons of a message.
    pub async fn message_buttons(&self, chat_id: i64, msg_id: i64) -> Result<Vec<ButtonInfo>> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;
        let msg = self
            .fetch_message_by_id(peer_ref, msg_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Message {} not found in chat {}", msg_id, chat_id))?;
        match msg.reply_markup() {
            Some(m) => Ok(extract_buttons(&m)),
            None => Ok(Vec::new()),
        }
    }

    /// Poll the chat for messages newer than `baseline`, up to `timeout_secs`.
    /// When `sender_filter` is set, only messages from that sender are kept, so
    /// in a busy group another user's message isn't mistaken for the bot's
    /// follow-up. Returns them oldest-first. Empty if nothing arrived in time.
    async fn wait_for_new_messages(
        &self,
        peer_ref: PeerRef,
        baseline: i32,
        timeout_secs: u64,
        sender_filter: Option<i64>,
    ) -> Result<Vec<Message>> {
        let deadline = Instant::now() + Duration::from_secs(timeout_secs);
        loop {
            // Fetch a generous window so we don't miss follow-ups when several
            // messages land between polls.
            let mut it = self.tg.client.iter_messages(peer_ref).limit(50);
            let mut newer = Vec::new();
            while let Some(m) = it.next().await? {
                if m.id() <= baseline {
                    break;
                }
                let from = m.sender().map(|p| p.id().bare_id());
                if sender_filter.is_none() || from == sender_filter {
                    newer.push(m);
                }
            }
            if !newer.is_empty() {
                newer.reverse();
                return Ok(newer);
            }
            if Instant::now() >= deadline {
                return Ok(Vec::new());
            }
            tokio::time::sleep(Duration::from_millis(1500)).await;
        }
    }

    /// Click an inline button on a bot message.
    ///
    /// The button is chosen by `button_idx` (see [`App::message_buttons`]) or by
    /// raw `data_b64` (URL-safe base64 callback payload). After answering, if
    /// `download` is set or `wait` is given, polls the chat for the bot's
    /// follow-up message(s) and optionally downloads their media.
    #[allow(clippy::too_many_arguments)]
    pub async fn click_button(
        &self,
        chat_id: i64,
        msg_id: i64,
        button_idx: Option<usize>,
        data_b64: Option<&str>,
        wait: Option<u64>,
        download: bool,
        dest: Option<&str>,
    ) -> Result<ClickOutcome> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;

        // Always fetch the origin message first: this validates `msg_id` exists
        // (even on the `--data` path) and gives us the sender to filter the
        // bot's follow-up by, so another user's message in a busy group isn't
        // mistaken for the reply.
        let origin = self
            .fetch_message_by_id(peer_ref, msg_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Message {} not found in chat {}", msg_id, chat_id))?;
        let origin_sender = origin.sender().map(|p| p.id().bare_id());

        // Resolve the callback payload to send.
        let data: Vec<u8> = if let Some(b64) = data_b64 {
            URL_SAFE_NO_PAD
                .decode(b64.trim())
                .context("Invalid --data: expected URL-safe base64 (no padding)")?
        } else if let Some(idx) = button_idx {
            let markup = origin
                .reply_markup()
                .ok_or_else(|| anyhow::anyhow!("Message {} has no inline buttons", msg_id))?;
            let buttons = extract_buttons(&markup);
            let b = buttons.get(idx).ok_or_else(|| {
                anyhow::anyhow!(
                    "Button index {} out of range (message has {} buttons)",
                    idx,
                    buttons.len()
                )
            })?;
            match (&b.kind[..], &b.data) {
                ("callback", Some(d)) => URL_SAFE_NO_PAD
                    .decode(d)
                    .expect("internal: button data is valid base64"),
                _ => {
                    let hint = b
                        .url
                        .as_ref()
                        .map(|u| format!(" (URL: {})", u))
                        .unwrap_or_default();
                    anyhow::bail!(
                        "Button {} ('{}') is a '{}' button, not a callback button{}",
                        idx,
                        b.text,
                        b.kind,
                        hint
                    );
                }
            }
        } else {
            anyhow::bail!("Specify --button <index> or --data <base64>");
        };

        // Baseline for detecting the bot's follow-up.
        let baseline = self.top_message_id(peer_ref).await?;

        // Press the button.
        let input_peer: tl::enums::InputPeer = peer_ref.into();
        let request = tl::functions::messages::GetBotCallbackAnswer {
            game: false,
            peer: input_peer,
            msg_id: msg_id as i32,
            data: Some(data),
            password: None,
        };
        let answer = match self.tg.client.invoke(&request).await {
            Ok(tl::enums::messages::BotCallbackAnswer::Answer(a)) => Some(a),
            Err(e) if e.to_string().contains("BOT_RESPONSE_TIMEOUT") => {
                // The bot received the callback but didn't answer it within
                // Telegram's window. Bots that do heavy work (e.g. fetching a
                // file) frequently still act on the press, so don't fail here —
                // fall through and look for the bot's follow-up message.
                eprintln!(
                    "note: bot did not answer the callback in time (BOT_RESPONSE_TIMEOUT); \
                     checking for a reply anyway"
                );
                None
            }
            Err(e) => {
                return Err(anyhow::Error::new(e).context("messages.getBotCallbackAnswer failed"));
            }
        };

        let mut outcome = ClickOutcome {
            message: answer.as_ref().and_then(|a| a.message.clone()),
            url: answer.as_ref().and_then(|a| a.url.clone()),
            alert: answer.as_ref().map(|a| a.alert).unwrap_or(false),
            new_messages: Vec::new(),
            downloaded: Vec::new(),
        };

        // Optionally wait for the bot's follow-up message(s).
        if download || wait.is_some() {
            let secs = wait.unwrap_or(30);
            let newer = self
                .wait_for_new_messages(peer_ref, baseline, secs, origin_sender)
                .await?;
            for m in &newer {
                outcome.new_messages.push(NewMessageInfo {
                    id: m.id() as i64,
                    has_media: m.media().is_some(),
                    text: m.text().chars().take(120).collect(),
                });
            }
            if download {
                for m in &newer {
                    if m.media().is_some() {
                        let r = self.download_media(chat_id, m.id() as i64, dest).await?;
                        outcome.downloaded.push(r.path);
                    }
                }
            }
        }

        Ok(outcome)
    }
}
