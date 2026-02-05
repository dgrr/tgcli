use crate::app::App;
use crate::store::UpsertMessageParams;
use anyhow::Result;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::Utc;
use grammers_client::InputMessage;
use grammers_session::defs::PeerRef;
use grammers_tl_types as tl;

/// Decode a file_id string back to its components.
/// Returns (doc_id, access_hash, file_reference)
fn decode_file_id(file_id: &str) -> Result<(i64, i64, Vec<u8>)> {
    let parts: Vec<&str> = file_id.split(':').collect();
    if parts.len() != 3 {
        anyhow::bail!("Invalid file_id format. Expected doc_id:access_hash:file_ref_base64");
    }
    let doc_id: i64 = parts[0].parse()?;
    let access_hash: i64 = parts[1].parse()?;
    let file_reference = URL_SAFE_NO_PAD.decode(parts[2])?;
    Ok((doc_id, access_hash, file_reference))
}

impl App {
    /// Send a text message to a chat by ID, returns the message ID.
    pub async fn send_text(&mut self, chat_id: i64, text: &str) -> Result<i64> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;
        let msg = self
            .tg
            .client
            .send_message(peer_ref, InputMessage::new().text(text))
            .await?;

        let now = Utc::now();
        self.store
            .upsert_message(UpsertMessageParams {
                id: msg.id() as i64,
                chat_id,
                sender_id: 0,
                ts: now,
                edit_ts: None,
                from_me: true,
                text: text.to_string(),
                media_type: None,
                media_path: None,
                reply_to_id: None,
                topic_id: None,
            })
            .await?;

        // Update chat's last_message_ts
        self.store
            .upsert_chat(chat_id, "user", "", None, Some(now), false)
            .await?;

        Ok(msg.id() as i64)
    }

    /// Mark a chat as read.
    pub async fn mark_read(&mut self, chat_id: i64) -> Result<()> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;
        self.tg.client.mark_as_read(peer_ref).await?;
        Ok(())
    }

    /// Delete messages from a chat.
    /// Returns the number of affected messages.
    /// Note: revoke is effectively always true (grammers hardcodes it).
    /// Delete messages from a chat. Always deletes for everyone (revoke=true).
    pub async fn delete_messages(&self, chat_id: i64, msg_ids: &[i64]) -> Result<usize> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;
        // grammers expects i32 message IDs
        let ids: Vec<i32> = msg_ids.iter().map(|&id| id as i32).collect();
        let affected = self.tg.client.delete_messages(peer_ref, &ids).await?;
        Ok(affected)
    }

    /// Send a sticker to a chat by ID, returns the message ID.
    pub async fn send_sticker(&mut self, chat_id: i64, sticker_file_id: &str) -> Result<i64> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;

        // Decode the file_id to get document components
        let (doc_id, access_hash, file_reference) = decode_file_id(sticker_file_id)?;

        // Create InputDocument for the sticker
        let input_doc = tl::enums::InputDocument::Document(tl::types::InputDocument {
            id: doc_id,
            access_hash,
            file_reference,
        });

        // Create InputMediaDocument for sending
        let input_media = tl::enums::InputMedia::Document(tl::types::InputMediaDocument {
            spoiler: false,
            id: input_doc,
            ttl_seconds: None,
            query: None,
            video_cover: None,
            video_timestamp: None,
        });

        // Send the sticker using InputMessage with media
        let msg = self
            .tg
            .client
            .send_message(peer_ref, InputMessage::new().text("").media(input_media))
            .await?;

        let now = Utc::now();
        self.store
            .upsert_message(UpsertMessageParams {
                id: msg.id() as i64,
                chat_id,
                sender_id: 0,
                ts: now,
                edit_ts: None,
                from_me: true,
                text: String::new(),
                media_type: Some("sticker".to_string()),
                media_path: None,
                reply_to_id: None,
                topic_id: None,
            })
            .await?;

        // Update chat's last_message_ts
        self.store
            .upsert_chat(chat_id, "user", "", None, Some(now), false)
            .await?;

        Ok(msg.id() as i64)
    }

    /// Resolve a chat ID to a PeerRef we can use for API calls.
    /// Iterates dialogs to find the matching peer.
    async fn resolve_peer_ref(&self, chat_id: i64) -> Result<PeerRef> {
        let mut dialogs = self.tg.client.iter_dialogs();
        while let Some(dialog) = dialogs.next().await? {
            let peer = dialog.peer();
            if peer.id().bare_id() == chat_id {
                return Ok(PeerRef::from(peer));
            }
        }
        anyhow::bail!("Chat {} not found. Make sure you've synced first.", chat_id);
    }
}
