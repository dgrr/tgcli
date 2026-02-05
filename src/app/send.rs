use crate::app::App;
use crate::store::UpsertMessageParams;
use anyhow::Result;
use chrono::Utc;
use grammers_client::InputMessage;
use grammers_session::defs::PeerRef;

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
                reply_to_id: None,
            })
            .await?;

        // Update chat's last_message_ts
        self.store
            .upsert_chat(chat_id, "user", "", None, Some(now))
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
    pub async fn delete_messages(
        &self,
        chat_id: i64,
        msg_ids: &[i64],
        _revoke: bool,
    ) -> Result<usize> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;
        // grammers expects i32 message IDs
        let ids: Vec<i32> = msg_ids.iter().map(|&id| id as i32).collect();
        let affected = self.tg.client.delete_messages(peer_ref, &ids).await?;
        Ok(affected)
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
