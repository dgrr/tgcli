use crate::app::App;
use crate::error::TgErrorContext;
use crate::store::UpsertMessageParams;
use anyhow::{Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::Utc;
use grammers_client::types::Attribute;
use grammers_client::InputMessage;
use grammers_session::defs::PeerRef;
use grammers_tl_types as tl;
use rand::Rng;
use std::path::Path;
use std::time::Duration;
use tl::enums::SendMessageAction;

/// Result from searching chats via Telegram API.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchChatResult {
    pub id: i64,
    pub kind: String,
    pub name: String,
    pub username: Option<String>,
}

/// Decode a file_id string back to its components.
/// Returns (doc_id, access_hash, file_reference)
fn decode_file_id(file_id: &str) -> Result<(i64, i64, Vec<u8>)> {
    let parts: Vec<&str> = file_id.split(':').collect();
    if parts.len() != 3 {
        anyhow::bail!("Invalid sticker file_id format. Use `tgcli stickers show --pack <pack_name>` to get valid file IDs.");
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
            .await
            .context_send(chat_id)?;

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
            .upsert_chat(chat_id, "user", "", None, Some(now), false, None)
            .await?;

        Ok(msg.id() as i64)
    }

    /// Send a scheduled text message to a chat by ID, returns the message ID.
    /// The message will be sent at the specified time (server-side scheduling).
    pub async fn send_text_scheduled(
        &mut self,
        chat_id: i64,
        text: &str,
        schedule_time: chrono::DateTime<Utc>,
    ) -> Result<i64> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;
        let input_peer: tl::enums::InputPeer = peer_ref.into();

        let random_id: i64 = rand::rng().random();
        let schedule_date = schedule_time.timestamp() as i32;

        let request = tl::functions::messages::SendMessage {
            no_webpage: true,
            silent: false,
            background: false,
            clear_draft: false,
            noforwards: false,
            update_stickersets_order: false,
            invert_media: false,
            allow_paid_floodskip: false,
            peer: input_peer,
            reply_to: None,
            message: text.to_string(),
            random_id,
            reply_markup: None,
            entities: None,
            schedule_date: Some(schedule_date),
            send_as: None,
            quick_reply_shortcut: None,
            effect: None,
            allow_paid_stars: None,
            suggested_post: None,
        };

        let updates = self
            .tg
            .client
            .invoke(&request)
            .await
            .context_send(chat_id)?;
        let msg_id = Self::extract_message_id_from_updates(&updates)?;

        // Note: We don't store scheduled messages in the local DB since they haven't been sent yet.
        // They will appear when the sync process picks them up after they're actually sent.

        Ok(msg_id)
    }

    /// Send a text message as a reply to another message, returns the message ID.
    pub async fn send_text_reply(
        &mut self,
        chat_id: i64,
        text: &str,
        reply_to_msg_id: i32,
    ) -> Result<i64> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;
        let input_peer: tl::enums::InputPeer = peer_ref.into();

        let random_id: i64 = rand::rng().random();

        let request = tl::functions::messages::SendMessage {
            no_webpage: true,
            silent: false,
            background: false,
            clear_draft: false,
            noforwards: false,
            update_stickersets_order: false,
            invert_media: false,
            allow_paid_floodskip: false,
            peer: input_peer,
            reply_to: Some(
                tl::types::InputReplyToMessage {
                    reply_to_msg_id,
                    top_msg_id: None,
                    reply_to_peer_id: None,
                    quote_text: None,
                    quote_entities: None,
                    quote_offset: None,
                    monoforum_peer_id: None,
                    todo_item_id: None,
                }
                .into(),
            ),
            message: text.to_string(),
            random_id,
            reply_markup: None,
            entities: None,
            schedule_date: None,
            send_as: None,
            quick_reply_shortcut: None,
            effect: None,
            allow_paid_stars: None,
            suggested_post: None,
        };

        let updates = self
            .tg
            .client
            .invoke(&request)
            .await
            .context_send(chat_id)?;
        let msg_id = Self::extract_message_id_from_updates(&updates)?;

        let now = Utc::now();
        self.store
            .upsert_message(UpsertMessageParams {
                id: msg_id,
                chat_id,
                sender_id: 0,
                ts: now,
                edit_ts: None,
                from_me: true,
                text: text.to_string(),
                media_type: None,
                media_path: None,
                reply_to_id: Some(reply_to_msg_id as i64),
                topic_id: None,
            })
            .await?;

        // Update chat's last_message_ts
        self.store
            .upsert_chat(chat_id, "user", "", None, Some(now), false, None)
            .await?;

        Ok(msg_id)
    }

    /// Send a text message to a specific forum topic by ID, returns the message ID.
    /// Uses raw TL invocation to set top_msg_id for topic support.
    pub async fn send_text_to_topic(
        &mut self,
        chat_id: i64,
        topic_id: i32,
        text: &str,
    ) -> Result<i64> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;
        let input_peer: tl::enums::InputPeer = peer_ref.into();

        let random_id: i64 = rand::rng().random();

        let request = tl::functions::messages::SendMessage {
            no_webpage: true,
            silent: false,
            background: false,
            clear_draft: false,
            noforwards: false,
            update_stickersets_order: false,
            invert_media: false,
            allow_paid_floodskip: false,
            peer: input_peer,
            reply_to: Some(
                tl::types::InputReplyToMessage {
                    reply_to_msg_id: topic_id,
                    top_msg_id: Some(topic_id),
                    reply_to_peer_id: None,
                    quote_text: None,
                    quote_entities: None,
                    quote_offset: None,
                    monoforum_peer_id: None,
                    todo_item_id: None,
                }
                .into(),
            ),
            message: text.to_string(),
            random_id,
            reply_markup: None,
            entities: None,
            schedule_date: None,
            send_as: None,
            quick_reply_shortcut: None,
            effect: None,
            allow_paid_stars: None,
            suggested_post: None,
        };

        let updates = self
            .tg
            .client
            .invoke(&request)
            .await
            .context_send(chat_id)?;

        // Extract message ID from updates
        let msg_id = Self::extract_message_id_from_updates(&updates)?;

        let now = Utc::now();
        self.store
            .upsert_message(UpsertMessageParams {
                id: msg_id,
                chat_id,
                sender_id: 0,
                ts: now,
                edit_ts: None,
                from_me: true,
                text: text.to_string(),
                media_type: None,
                media_path: None,
                reply_to_id: None,
                topic_id: Some(topic_id),
            })
            .await?;

        // Update chat's last_message_ts
        self.store
            .upsert_chat(chat_id, "user", "", None, Some(now), false, None)
            .await?;

        Ok(msg_id)
    }

    /// Extract message ID from Updates response
    fn extract_message_id_from_updates(updates: &tl::enums::Updates) -> Result<i64> {
        match updates {
            tl::enums::Updates::Updates(u) => {
                for update in &u.updates {
                    if let tl::enums::Update::NewMessage(m) = update {
                        if let tl::enums::Message::Message(msg) = &m.message {
                            return Ok(msg.id as i64);
                        }
                    }
                    if let tl::enums::Update::NewChannelMessage(m) = update {
                        if let tl::enums::Message::Message(msg) = &m.message {
                            return Ok(msg.id as i64);
                        }
                    }
                }
                anyhow::bail!("No message ID found in Updates response")
            }
            tl::enums::Updates::UpdateShort(u) => {
                if let tl::enums::Update::NewMessage(m) = &u.update {
                    if let tl::enums::Message::Message(msg) = &m.message {
                        return Ok(msg.id as i64);
                    }
                }
                anyhow::bail!("No message ID found in UpdateShort response")
            }
            tl::enums::Updates::UpdateShortMessage(u) => Ok(u.id as i64),
            tl::enums::Updates::UpdateShortChatMessage(u) => Ok(u.id as i64),
            tl::enums::Updates::UpdateShortSentMessage(u) => Ok(u.id as i64),
            _ => anyhow::bail!("Unexpected Updates type"),
        }
    }

    /// Pin a message in a chat.
    pub async fn pin_message(
        &self,
        chat_id: i64,
        msg_id: i64,
        silent: bool,
        pm_oneside: bool,
    ) -> Result<()> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;
        let input_peer: tl::enums::InputPeer = peer_ref.into();

        let request = tl::functions::messages::UpdatePinnedMessage {
            silent,
            unpin: false,
            pm_oneside,
            peer: input_peer,
            id: msg_id as i32,
        };

        self.tg
            .client
            .invoke(&request)
            .await
            .context_pin(chat_id, msg_id, true)?;
        Ok(())
    }

    /// Unpin a message in a chat.
    pub async fn unpin_message(&self, chat_id: i64, msg_id: i64, pm_oneside: bool) -> Result<()> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;
        let input_peer: tl::enums::InputPeer = peer_ref.into();

        let request = tl::functions::messages::UpdatePinnedMessage {
            silent: true,
            unpin: true,
            pm_oneside,
            peer: input_peer,
            id: msg_id as i32,
        };

        self.tg.client.invoke(&request).await.context(format!(
            "Failed to unpin message {} in chat {}",
            msg_id, chat_id
        ))?;
        Ok(())
    }

    /// Edit a message's text.
    pub async fn edit_message(&self, chat_id: i64, msg_id: i64, new_text: &str) -> Result<()> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;
        let input_peer: tl::enums::InputPeer = peer_ref.into();

        let request = tl::functions::messages::EditMessage {
            no_webpage: true,
            invert_media: false,
            peer: input_peer,
            id: msg_id as i32,
            message: Some(new_text.to_string()),
            media: None,
            reply_markup: None,
            entities: None,
            schedule_date: None,
            quick_reply_shortcut_id: None,
        };

        self.tg.client.invoke(&request).await.context(format!(
            "Failed to edit message {} in chat {}",
            msg_id, chat_id
        ))?;

        // Update local store
        self.store
            .update_message_text(chat_id, msg_id, new_text)
            .await?;

        Ok(())
    }

    /// Forward a message from one chat to another (optionally to a specific topic).
    /// Returns the new message ID in the destination chat.
    pub async fn forward_message(
        &self,
        from_chat_id: i64,
        msg_id: i64,
        to_chat_id: i64,
        to_topic_id: Option<i32>,
    ) -> Result<i64> {
        let from_peer = self.resolve_peer_ref(from_chat_id).await?;
        let to_peer = self.resolve_peer_ref(to_chat_id).await?;

        let from_input_peer: tl::enums::InputPeer = from_peer.into();
        let to_input_peer: tl::enums::InputPeer = to_peer.into();

        let random_id: i64 = rand::rng().random();

        let request = tl::functions::messages::ForwardMessages {
            silent: false,
            background: false,
            with_my_score: false,
            drop_author: false,
            drop_media_captions: false,
            noforwards: false,
            allow_paid_floodskip: false,
            from_peer: from_input_peer,
            id: vec![msg_id as i32],
            random_id: vec![random_id],
            to_peer: to_input_peer,
            top_msg_id: to_topic_id,
            schedule_date: None,
            send_as: None,
            quick_reply_shortcut: None,
            video_timestamp: None,
            allow_paid_stars: None,
            reply_to: None,
            suggested_post: None,
        };

        let updates = self.tg.client.invoke(&request).await.context(format!(
            "Failed to forward message {} from chat {} to chat {}",
            msg_id, from_chat_id, to_chat_id
        ))?;
        let new_msg_id = Self::extract_message_id_from_updates(&updates)?;

        Ok(new_msg_id)
    }

    /// Mark a chat (or topic in a forum) as read.
    pub async fn mark_read(&self, chat_id: i64, topic_id: Option<i32>) -> Result<()> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;

        if let Some(tid) = topic_id {
            // For forum topics, use ReadDiscussion to mark the topic as read
            let input_peer: tl::enums::InputPeer = peer_ref.into();
            let request = tl::functions::messages::ReadDiscussion {
                peer: input_peer,
                msg_id: tid,
                read_max_id: i32::MAX,
            };
            self.tg.client.invoke(&request).await.context(format!(
                "Failed to mark topic {} in chat {} as read",
                tid, chat_id
            ))?;
        } else {
            self.tg
                .client
                .mark_as_read(peer_ref)
                .await
                .context(format!("Failed to mark chat {} as read", chat_id))?;
        }
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
        let affected = self
            .tg
            .client
            .delete_messages(peer_ref, &ids)
            .await
            .context(format!(
                "Failed to delete {} message(s) from chat {}",
                msg_ids.len(),
                chat_id
            ))?;
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
            .await
            .context(format!("Failed to send sticker to chat {}", chat_id))?;

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
            .upsert_chat(chat_id, "user", "", None, Some(now), false, None)
            .await?;

        Ok(msg.id() as i64)
    }

    /// Send a photo to a chat by ID, returns the message ID.
    pub async fn send_photo(&mut self, chat_id: i64, path: &Path, caption: &str) -> Result<i64> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;

        // Upload the file
        let uploaded = self
            .tg
            .client
            .upload_file(path)
            .await
            .context(format!("Failed to upload photo '{}'", path.display()))?;

        // Send as photo with caption
        let msg = self
            .tg
            .client
            .send_message(peer_ref, InputMessage::new().text(caption).photo(uploaded))
            .await
            .context(format!("Failed to send photo to chat {}", chat_id))?;

        let now = Utc::now();
        self.store
            .upsert_message(UpsertMessageParams {
                id: msg.id() as i64,
                chat_id,
                sender_id: 0,
                ts: now,
                edit_ts: None,
                from_me: true,
                text: caption.to_string(),
                media_type: Some("photo".to_string()),
                media_path: Some(path.to_string_lossy().to_string()),
                reply_to_id: None,
                topic_id: None,
            })
            .await?;

        // Update chat's last_message_ts
        self.store
            .upsert_chat(chat_id, "user", "", None, Some(now), false, None)
            .await?;

        Ok(msg.id() as i64)
    }

    /// Send a video to a chat by ID, returns the message ID.
    pub async fn send_video(&mut self, chat_id: i64, path: &Path, caption: &str) -> Result<i64> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;

        // Upload the file
        let uploaded = self
            .tg
            .client
            .upload_file(path)
            .await
            .context(format!("Failed to upload video '{}'", path.display()))?;

        // Send as document with video attribute
        let msg = self
            .tg
            .client
            .send_message(
                peer_ref,
                InputMessage::new()
                    .text(caption)
                    .document(uploaded)
                    .attribute(Attribute::Video {
                        round_message: false,
                        supports_streaming: true,
                        duration: Duration::from_secs(0), // Duration unknown
                        w: 0,
                        h: 0,
                    }),
            )
            .await
            .context(format!("Failed to send video to chat {}", chat_id))?;

        let now = Utc::now();
        self.store
            .upsert_message(UpsertMessageParams {
                id: msg.id() as i64,
                chat_id,
                sender_id: 0,
                ts: now,
                edit_ts: None,
                from_me: true,
                text: caption.to_string(),
                media_type: Some("video".to_string()),
                media_path: Some(path.to_string_lossy().to_string()),
                reply_to_id: None,
                topic_id: None,
            })
            .await?;

        // Update chat's last_message_ts
        self.store
            .upsert_chat(chat_id, "user", "", None, Some(now), false, None)
            .await?;

        Ok(msg.id() as i64)
    }

    /// Send a file as document to a chat by ID, returns the message ID.
    /// Preserves the original filename.
    pub async fn send_file(&mut self, chat_id: i64, path: &Path, caption: &str) -> Result<i64> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;

        // Upload the file
        let uploaded = self
            .tg
            .client
            .upload_file(path)
            .await
            .context(format!("Failed to upload file '{}'", path.display()))?;

        // Send as document (grammers automatically preserves filename)
        let msg = self
            .tg
            .client
            .send_message(
                peer_ref,
                InputMessage::new().text(caption).document(uploaded),
            )
            .await
            .context(format!("Failed to send file to chat {}", chat_id))?;

        let now = Utc::now();
        self.store
            .upsert_message(UpsertMessageParams {
                id: msg.id() as i64,
                chat_id,
                sender_id: 0,
                ts: now,
                edit_ts: None,
                from_me: true,
                text: caption.to_string(),
                media_type: Some("document".to_string()),
                media_path: Some(path.to_string_lossy().to_string()),
                reply_to_id: None,
                topic_id: None,
            })
            .await?;

        // Update chat's last_message_ts
        self.store
            .upsert_chat(chat_id, "user", "", None, Some(now), false, None)
            .await?;

        Ok(msg.id() as i64)
    }

    /// Send an audio file as a voice message to a chat by ID, returns the message ID.
    /// Voice messages play inline in Telegram clients.
    pub async fn send_voice(&mut self, chat_id: i64, path: &Path, caption: &str) -> Result<i64> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;

        // Upload the file
        let uploaded = self
            .tg
            .client
            .upload_file(path)
            .await
            .context(format!("Failed to upload voice file '{}'", path.display()))?;

        // Send as document with voice attribute
        let msg = self
            .tg
            .client
            .send_message(
                peer_ref,
                InputMessage::new()
                    .text(caption)
                    .document(uploaded)
                    .attribute(Attribute::Voice {
                        duration: Duration::from_secs(0), // Duration unknown, Telegram will detect
                        waveform: None,
                    }),
            )
            .await
            .context(format!("Failed to send voice message to chat {}", chat_id))?;

        let now = Utc::now();
        self.store
            .upsert_message(UpsertMessageParams {
                id: msg.id() as i64,
                chat_id,
                sender_id: 0,
                ts: now,
                edit_ts: None,
                from_me: true,
                text: caption.to_string(),
                media_type: Some("voice".to_string()),
                media_path: Some(path.to_string_lossy().to_string()),
                reply_to_id: None,
                topic_id: None,
            })
            .await?;

        // Update chat's last_message_ts
        self.store
            .upsert_chat(chat_id, "user", "", None, Some(now), false, None)
            .await?;

        Ok(msg.id() as i64)
    }

    /// Send or remove a reaction on a message.
    /// If `remove` is true, removes the specified reaction. Otherwise, adds it.
    pub async fn send_reaction(
        &self,
        chat_id: i64,
        msg_id: i64,
        emoji: &str,
        remove: bool,
    ) -> Result<()> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;
        let input_peer: tl::enums::InputPeer = peer_ref.into();

        // Build the reaction vector
        let reaction = if remove {
            // Empty vector or None removes the reaction
            None
        } else {
            Some(vec![tl::enums::Reaction::Emoji(tl::types::ReactionEmoji {
                emoticon: emoji.to_string(),
            })])
        };

        let request = tl::functions::messages::SendReaction {
            big: false,
            add_to_recent: true,
            peer: input_peer,
            msg_id: msg_id as i32,
            reaction,
        };

        self.tg.client.invoke(&request).await.context(format!(
            "Failed to {} reaction {} on message {} in chat {}",
            if remove { "remove" } else { "add" },
            emoji,
            msg_id,
            chat_id
        ))?;

        Ok(())
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
        anyhow::bail!(
            "Chat {} not found. Run `tgcli sync` to refresh your chat list.",
            chat_id
        );
    }

    /// Backfill (fetch older) messages for a chat.
    /// Fetches messages older than `offset_id` (going backwards in time).
    /// If `offset_id` is None, fetches from the latest messages.
    /// Returns the number of new messages fetched and stored.
    #[allow(dead_code)]
    pub async fn backfill_messages(
        &self,
        chat_id: i64,
        topic_id: Option<i32>,
        offset_id: Option<i64>,
        limit: usize,
    ) -> Result<usize> {
        self.backfill_messages_with_progress(chat_id, topic_id, offset_id, limit, false)
            .await
    }

    /// Backfill messages with optional progress output.
    pub async fn backfill_messages_with_progress(
        &self,
        chat_id: i64,
        topic_id: Option<i32>,
        offset_id: Option<i64>,
        limit: usize,
        show_progress: bool,
    ) -> Result<usize> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;

        // Check if this chat is a forum
        let chat = self.store.get_chat(chat_id).await?;
        let is_forum = chat.map(|c| c.is_forum).unwrap_or(false);

        let mut message_iter = self.tg.client.iter_messages(peer_ref);

        // Set offset_id if provided (fetch messages older than this)
        if let Some(oid) = offset_id {
            message_iter = message_iter.offset_id(oid as i32);
        }

        // Progress tracking
        let progress_interval = std::time::Duration::from_millis(500);
        let mut last_progress_time = std::time::Instant::now();

        if show_progress {
            eprint!("\rFetching... 0/{} messages", limit);
        }

        let mut count = 0;
        while let Some(msg) = message_iter.next().await? {
            if count >= limit {
                break;
            }

            // If fetching for a specific topic, filter messages
            let msg_topic_id = if is_forum {
                extract_topic_id_from_raw(&msg.raw)
            } else {
                None
            };

            if topic_id.is_some() && msg_topic_id != topic_id {
                continue;
            }

            let sender_id = msg.sender().map(|s| s.id().bare_id()).unwrap_or(0);
            let from_me = msg.outgoing();
            let text = msg.text().to_string();
            let reply_to_id = msg.reply_to_message_id().map(|id| id as i64);
            let media_type = msg.media().map(|_| "media".to_string());

            self.store
                .upsert_message(UpsertMessageParams {
                    id: msg.id() as i64,
                    chat_id,
                    sender_id,
                    ts: msg.date(),
                    edit_ts: msg.edit_date(),
                    from_me,
                    text,
                    media_type,
                    media_path: None,
                    reply_to_id,
                    topic_id: msg_topic_id,
                })
                .await?;
            count += 1;

            // Show progress periodically
            if show_progress && last_progress_time.elapsed() >= progress_interval {
                eprint!("\rFetching... {}/{} messages", count, limit);
                last_progress_time = std::time::Instant::now();
            }
        }

        if show_progress {
            // Clear progress line
            eprint!("\r\x1b[K");
        }

        Ok(count)
    }

    /// Send a poll to a chat by ID, returns the message ID.
    pub async fn send_poll(
        &mut self,
        chat_id: i64,
        question: &str,
        options: &[String],
        multiple_choice: bool,
        public_voters: bool,
    ) -> Result<i64> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;
        let input_peer: tl::enums::InputPeer = peer_ref.into();

        // Generate a random poll ID
        let poll_id: i64 = rand::rng().random();

        // Build poll answers with unique option identifiers
        let answers: Vec<tl::enums::PollAnswer> = options
            .iter()
            .enumerate()
            .map(|(i, text)| {
                tl::enums::PollAnswer::Answer(tl::types::PollAnswer {
                    text: tl::enums::TextWithEntities::Entities(tl::types::TextWithEntities {
                        text: text.clone(),
                        entities: vec![],
                    }),
                    option: vec![i as u8], // Use index as option identifier
                })
            })
            .collect();

        // Build the poll
        let poll = tl::enums::Poll::Poll(tl::types::Poll {
            id: poll_id,
            closed: false,
            public_voters,
            multiple_choice,
            quiz: false,
            question: tl::enums::TextWithEntities::Entities(tl::types::TextWithEntities {
                text: question.to_string(),
                entities: vec![],
            }),
            answers,
            close_period: None,
            close_date: None,
        });

        // Create InputMediaPoll
        let input_media = tl::enums::InputMedia::Poll(tl::types::InputMediaPoll {
            poll,
            correct_answers: None,
            solution: None,
            solution_entities: None,
        });

        let random_id: i64 = rand::rng().random();

        let request = tl::functions::messages::SendMedia {
            silent: false,
            background: false,
            clear_draft: false,
            noforwards: false,
            update_stickersets_order: false,
            invert_media: false,
            allow_paid_floodskip: false,
            peer: input_peer,
            reply_to: None,
            media: input_media,
            message: String::new(),
            random_id,
            reply_markup: None,
            entities: None,
            schedule_date: None,
            send_as: None,
            quick_reply_shortcut: None,
            effect: None,
            allow_paid_stars: None,
            suggested_post: None,
        };

        let updates = self
            .tg
            .client
            .invoke(&request)
            .await
            .context(format!("Failed to send poll to chat {}", chat_id))?;

        let msg_id = Self::extract_message_id_from_updates(&updates)?;

        let now = Utc::now();
        self.store
            .upsert_message(UpsertMessageParams {
                id: msg_id,
                chat_id,
                sender_id: 0,
                ts: now,
                edit_ts: None,
                from_me: true,
                text: question.to_string(),
                media_type: Some("poll".to_string()),
                media_path: None,
                reply_to_id: None,
                topic_id: None,
            })
            .await?;

        // Update chat's last_message_ts
        self.store
            .upsert_chat(chat_id, "user", "", None, Some(now), false, None)
            .await?;

        Ok(msg_id)
    }

    /// Vote in a poll.
    pub async fn vote_poll(
        &self,
        chat_id: i64,
        msg_id: i64,
        option_indices: &[usize],
    ) -> Result<()> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;
        let input_peer: tl::enums::InputPeer = peer_ref.into();

        // Convert option indices to option bytes (each option is identified by its index as a single byte)
        let options: Vec<Vec<u8>> = option_indices.iter().map(|&i| vec![i as u8]).collect();

        let request = tl::functions::messages::SendVote {
            peer: input_peer,
            msg_id: msg_id as i32,
            options,
        };

        self.tg.client.invoke(&request).await.context(format!(
            "Failed to vote in poll (message {} in chat {})",
            msg_id, chat_id
        ))?;

        Ok(())
    }

    /// Send typing indicator to a chat (or topic in a forum).
    pub async fn set_typing(&self, chat_id: i64, topic_id: Option<i32>) -> Result<()> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;

        if let Some(tid) = topic_id {
            // For forum topics, use raw TL to set typing with top_msg_id
            let input_peer: tl::enums::InputPeer = peer_ref.into();
            let request = tl::functions::messages::SetTyping {
                peer: input_peer,
                top_msg_id: Some(tid),
                action: SendMessageAction::SendMessageTypingAction,
            };
            self.tg.client.invoke(&request).await.context(format!(
                "Failed to set typing indicator in topic {} of chat {}",
                tid, chat_id
            ))?;
        } else {
            self.tg
                .client
                .action(peer_ref)
                .oneshot(SendMessageAction::SendMessageTypingAction)
                .await
                .context(format!(
                    "Failed to set typing indicator in chat {}",
                    chat_id
                ))?;
        }
        Ok(())
    }

    /// Cancel typing indicator in a chat (or topic in a forum).
    pub async fn cancel_typing(&self, chat_id: i64, topic_id: Option<i32>) -> Result<()> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;

        if let Some(tid) = topic_id {
            // For forum topics, use raw TL to cancel typing with top_msg_id
            let input_peer: tl::enums::InputPeer = peer_ref.into();
            let request = tl::functions::messages::SetTyping {
                peer: input_peer,
                top_msg_id: Some(tid),
                action: SendMessageAction::SendMessageCancelAction,
            };
            self.tg.client.invoke(&request).await.context(format!(
                "Failed to cancel typing indicator in topic {} of chat {}",
                tid, chat_id
            ))?;
        } else {
            self.tg
                .client
                .action(peer_ref)
                .cancel()
                .await
                .context(format!(
                    "Failed to cancel typing indicator in chat {}",
                    chat_id
                ))?;
        }
        Ok(())
    }

    /// Ban a user from a group or channel.
    /// until_date: 0 = forever, otherwise Unix timestamp
    pub async fn ban_user(&self, chat_id: i64, user_id: i64, until_date: i32) -> Result<()> {
        let channel_peer = self.resolve_channel_input(chat_id).await?;
        let user_peer = self.resolve_user_input_peer(user_id).await?;

        let banned_rights = tl::types::ChatBannedRights {
            view_messages: true,
            send_messages: true,
            send_media: true,
            send_stickers: true,
            send_gifs: true,
            send_games: true,
            send_inline: true,
            embed_links: true,
            send_polls: true,
            change_info: true,
            invite_users: true,
            pin_messages: true,
            manage_topics: true,
            send_photos: true,
            send_videos: true,
            send_roundvideos: true,
            send_audios: true,
            send_voices: true,
            send_docs: true,
            send_plain: true,
            until_date,
        };

        let request = tl::functions::channels::EditBanned {
            channel: channel_peer,
            participant: user_peer,
            banned_rights: tl::enums::ChatBannedRights::Rights(banned_rights),
        };

        self.tg.client.invoke(&request).await.context(format!(
            "Failed to ban user {} from chat {}",
            user_id, chat_id
        ))?;

        Ok(())
    }

    /// Kick a user from a group or channel (they can rejoin).
    pub async fn kick_user(&self, chat_id: i64, user_id: i64) -> Result<()> {
        let channel_peer = self.resolve_channel_input(chat_id).await?;
        let user_peer = self.resolve_user_input_peer(user_id).await?;

        // Kick = ban then immediately unban
        let banned_rights = tl::types::ChatBannedRights {
            view_messages: true,
            send_messages: true,
            send_media: true,
            send_stickers: true,
            send_gifs: true,
            send_games: true,
            send_inline: true,
            embed_links: true,
            send_polls: true,
            change_info: true,
            invite_users: true,
            pin_messages: true,
            manage_topics: true,
            send_photos: true,
            send_videos: true,
            send_roundvideos: true,
            send_audios: true,
            send_voices: true,
            send_docs: true,
            send_plain: true,
            until_date: 0, // Permanent ban first
        };

        let request = tl::functions::channels::EditBanned {
            channel: channel_peer.clone(),
            participant: user_peer.clone(),
            banned_rights: tl::enums::ChatBannedRights::Rights(banned_rights),
        };

        self.tg.client.invoke(&request).await.context(format!(
            "Failed to kick user {} from chat {}",
            user_id, chat_id
        ))?;

        // Now unban so they can rejoin
        let unbanned_rights = tl::types::ChatBannedRights {
            view_messages: false,
            send_messages: false,
            send_media: false,
            send_stickers: false,
            send_gifs: false,
            send_games: false,
            send_inline: false,
            embed_links: false,
            send_polls: false,
            change_info: false,
            invite_users: false,
            pin_messages: false,
            manage_topics: false,
            send_photos: false,
            send_videos: false,
            send_roundvideos: false,
            send_audios: false,
            send_voices: false,
            send_docs: false,
            send_plain: false,
            until_date: 0,
        };

        let unban_request = tl::functions::channels::EditBanned {
            channel: channel_peer,
            participant: user_peer,
            banned_rights: tl::enums::ChatBannedRights::Rights(unbanned_rights),
        };

        self.tg
            .client
            .invoke(&unban_request)
            .await
            .context(format!(
                "Failed to unban user {} after kick from chat {}",
                user_id, chat_id
            ))?;

        Ok(())
    }

    /// Unban a user from a group or channel.
    pub async fn unban_user(&self, chat_id: i64, user_id: i64) -> Result<()> {
        let channel_peer = self.resolve_channel_input(chat_id).await?;
        let user_peer = self.resolve_user_input_peer(user_id).await?;

        let unbanned_rights = tl::types::ChatBannedRights {
            view_messages: false,
            send_messages: false,
            send_media: false,
            send_stickers: false,
            send_gifs: false,
            send_games: false,
            send_inline: false,
            embed_links: false,
            send_polls: false,
            change_info: false,
            invite_users: false,
            pin_messages: false,
            manage_topics: false,
            send_photos: false,
            send_videos: false,
            send_roundvideos: false,
            send_audios: false,
            send_voices: false,
            send_docs: false,
            send_plain: false,
            until_date: 0,
        };

        let request = tl::functions::channels::EditBanned {
            channel: channel_peer,
            participant: user_peer,
            banned_rights: tl::enums::ChatBannedRights::Rights(unbanned_rights),
        };

        self.tg.client.invoke(&request).await.context(format!(
            "Failed to unban user {} from chat {}",
            user_id, chat_id
        ))?;

        Ok(())
    }

    /// Promote a user to admin in a group or channel.
    pub async fn promote_user(
        &self,
        chat_id: i64,
        user_id: i64,
        title: Option<&str>,
    ) -> Result<()> {
        let channel_peer = self.resolve_channel_input(chat_id).await?;
        let user_peer = self.resolve_user_input(user_id).await?;

        // Grant typical moderator permissions
        let admin_rights = tl::types::ChatAdminRights {
            change_info: false,
            post_messages: false,
            edit_messages: false,
            delete_messages: true,
            ban_users: true,
            invite_users: true,
            pin_messages: true,
            add_admins: false,
            anonymous: false,
            manage_call: false,
            other: true,
            manage_topics: true,
            post_stories: false,
            edit_stories: false,
            delete_stories: false,
            manage_direct_messages: false,
        };

        let request = tl::functions::channels::EditAdmin {
            channel: channel_peer,
            user_id: user_peer,
            admin_rights: tl::enums::ChatAdminRights::Rights(admin_rights),
            rank: title.unwrap_or("Admin").to_string(),
        };

        self.tg.client.invoke(&request).await.context(format!(
            "Failed to promote user {} in chat {}",
            user_id, chat_id
        ))?;

        Ok(())
    }

    /// Demote an admin to regular user.
    pub async fn demote_user(&self, chat_id: i64, user_id: i64) -> Result<()> {
        let channel_peer = self.resolve_channel_input(chat_id).await?;
        let user_peer = self.resolve_user_input(user_id).await?;

        // Remove all admin rights
        let admin_rights = tl::types::ChatAdminRights {
            change_info: false,
            post_messages: false,
            edit_messages: false,
            delete_messages: false,
            ban_users: false,
            invite_users: false,
            pin_messages: false,
            add_admins: false,
            anonymous: false,
            manage_call: false,
            other: false,
            manage_topics: false,
            post_stories: false,
            edit_stories: false,
            delete_stories: false,
            manage_direct_messages: false,
        };

        let request = tl::functions::channels::EditAdmin {
            channel: channel_peer,
            user_id: user_peer,
            admin_rights: tl::enums::ChatAdminRights::Rights(admin_rights),
            rank: String::new(),
        };

        self.tg.client.invoke(&request).await.context(format!(
            "Failed to demote user {} in chat {}",
            user_id, chat_id
        ))?;

        Ok(())
    }

    /// Resolve a chat ID to InputChannel for admin operations.
    async fn resolve_channel_input(&self, chat_id: i64) -> Result<tl::enums::InputChannel> {
        // Resolve via dialogs (most reliable - gets fresh access_hash)
        let mut dialogs = self.tg.client.iter_dialogs();
        while let Some(dialog) = dialogs.next().await? {
            let peer = dialog.peer();
            if peer.id().bare_id() == chat_id {
                let peer_ref = PeerRef::from(peer);
                if let tl::enums::InputPeer::Channel(ch) = tl::enums::InputPeer::from(peer_ref) {
                    return Ok(tl::enums::InputChannel::Channel(tl::types::InputChannel {
                        channel_id: ch.channel_id,
                        access_hash: ch.access_hash,
                    }));
                }
            }
        }

        anyhow::bail!(
            "Chat {} not found or is not a channel/supergroup. Admin operations require a channel or supergroup.",
            chat_id
        );
    }

    /// Resolve a user ID to InputUser for admin operations.
    async fn resolve_user_input(&self, user_id: i64) -> Result<tl::enums::InputUser> {
        // Resolve via dialogs
        let mut dialogs = self.tg.client.iter_dialogs();
        while let Some(dialog) = dialogs.next().await? {
            let peer = dialog.peer();
            if peer.id().bare_id() == user_id {
                let peer_ref = PeerRef::from(peer);
                if let tl::enums::InputPeer::User(u) = tl::enums::InputPeer::from(peer_ref) {
                    return Ok(tl::enums::InputUser::User(tl::types::InputUser {
                        user_id: u.user_id,
                        access_hash: u.access_hash,
                    }));
                }
            }
        }

        anyhow::bail!(
            "User {} not found. Make sure the user is in your contacts or chat list. Run `tgcli sync` to refresh.",
            user_id
        );
    }

    /// Search for chats by name via Telegram API.
    /// Returns a list of matching chats (users, groups, channels).
    pub async fn search_chats(&self, query: &str, limit: usize) -> Result<Vec<SearchChatResult>> {
        let request = tl::functions::contacts::Search {
            q: query.to_string(),
            limit: limit as i32,
        };

        let result = self
            .tg
            .client
            .invoke(&request)
            .await
            .context(format!("Failed to search for '{}'", query))?;

        let (chats, users) = match result {
            tl::enums::contacts::Found::Found(f) => (f.chats, f.users),
        };

        let mut results = Vec::new();

        // Process users
        for user in users {
            if let tl::enums::User::User(u) = user {
                let name = format!(
                    "{} {}",
                    u.first_name.as_deref().unwrap_or(""),
                    u.last_name.as_deref().unwrap_or("")
                )
                .trim()
                .to_string();
                results.push(SearchChatResult {
                    id: u.id,
                    kind: "user".to_string(),
                    name,
                    username: u.username,
                });
            }
        }

        // Process chats
        for chat in chats {
            match chat {
                tl::enums::Chat::Chat(c) => {
                    results.push(SearchChatResult {
                        id: c.id,
                        kind: "group".to_string(),
                        name: c.title,
                        username: None,
                    });
                }
                tl::enums::Chat::Channel(c) => {
                    let kind = if c.broadcast { "channel" } else { "supergroup" };
                    results.push(SearchChatResult {
                        id: c.id,
                        kind: kind.to_string(),
                        name: c.title,
                        username: c.username,
                    });
                }
                _ => {}
            }
        }

        Ok(results)
    }

    /// Resolve a user ID to InputPeer for ban operations.
    async fn resolve_user_input_peer(&self, user_id: i64) -> Result<tl::enums::InputPeer> {
        // Resolve via dialogs
        let mut dialogs = self.tg.client.iter_dialogs();
        while let Some(dialog) = dialogs.next().await? {
            let peer = dialog.peer();
            if peer.id().bare_id() == user_id {
                let peer_ref = PeerRef::from(peer);
                return Ok(tl::enums::InputPeer::from(peer_ref));
            }
        }

        anyhow::bail!(
            "User {} not found. Make sure the user is in your contacts or chat list. Run `tgcli sync` to refresh.",
            user_id
        );
    }

    /// Global search across all chats via Telegram API.
    /// Returns messages matching the query.
    pub async fn global_search(
        &self,
        query: &str,
        chat_id: Option<i64>,
        limit: usize,
    ) -> Result<Vec<crate::store::Message>> {
        let mut results = Vec::new();

        if let Some(cid) = chat_id {
            // Search within a specific chat
            let peer_ref = self.resolve_peer_ref(cid).await?;
            let mut iter = self.tg.client.search_messages(peer_ref).query(query);
            let mut count = 0;

            while let Some(msg) = iter.next().await? {
                if count >= limit {
                    break;
                }
                count += 1;

                let sender_id = msg.sender().map(|s| s.id().bare_id()).unwrap_or(0);
                let from_me = msg.outgoing();

                results.push(crate::store::Message {
                    id: msg.id() as i64,
                    chat_id: cid,
                    sender_id,
                    ts: msg.date(),
                    edit_ts: msg.edit_date(),
                    from_me,
                    text: msg.text().to_string(),
                    media_type: msg.media().map(|_| "media".to_string()),
                    media_path: None,
                    reply_to_id: msg.reply_to_message_id().map(|id| id as i64),
                    topic_id: None,
                    snippet: String::new(),
                });
            }
        } else {
            // Global search across all chats
            let mut iter = self.tg.client.search_all_messages().query(query);
            let mut count = 0;

            while let Some(msg) = iter.next().await? {
                if count >= limit {
                    break;
                }
                count += 1;

                let sender_id = msg.sender().map(|s| s.id().bare_id()).unwrap_or(0);
                let from_me = msg.outgoing();
                let msg_chat_id = msg.peer_id().bare_id();

                results.push(crate::store::Message {
                    id: msg.id() as i64,
                    chat_id: msg_chat_id,
                    sender_id,
                    ts: msg.date(),
                    edit_ts: msg.edit_date(),
                    from_me,
                    text: msg.text().to_string(),
                    media_type: msg.media().map(|_| "media".to_string()),
                    media_path: None,
                    reply_to_id: msg.reply_to_message_id().map(|id| id as i64),
                    topic_id: None,
                    snippet: String::new(),
                });
            }
        }

        Ok(results)
    }

    /// Create a new group or channel.
    /// Returns a CreateChatResult with the new chat's info.
    pub async fn create_chat(
        &self,
        name: &str,
        chat_type: &str,
        description: Option<&str>,
    ) -> Result<CreateChatResult> {
        match chat_type {
            "group" => {
                // Create a supergroup (megagroup) which supports descriptions
                // Basic groups have limitations, so we create a supergroup instead
                let request = tl::functions::channels::CreateChannel {
                    broadcast: false,
                    megagroup: true, // This creates a supergroup
                    for_import: false,
                    forum: false,
                    title: name.to_string(),
                    about: description.unwrap_or("").to_string(),
                    geo_point: None,
                    address: None,
                    ttl_period: None,
                };

                let updates = self
                    .tg
                    .client
                    .invoke(&request)
                    .await
                    .context("Failed to create group")?;

                let chat_id = Self::extract_channel_id_from_updates(&updates)?;

                Ok(CreateChatResult {
                    id: chat_id,
                    kind: "group".to_string(),
                    name: name.to_string(),
                })
            }
            "channel" => {
                // Create a channel (actually creates a megagroup/supergroup by default)
                let request = tl::functions::channels::CreateChannel {
                    broadcast: true, // true = channel, false = megagroup
                    megagroup: false,
                    for_import: false,
                    forum: false,
                    title: name.to_string(),
                    about: description.unwrap_or("").to_string(),
                    geo_point: None,
                    address: None,
                    ttl_period: None,
                };

                let updates = self
                    .tg
                    .client
                    .invoke(&request)
                    .await
                    .context("Failed to create channel")?;

                let chat_id = Self::extract_channel_id_from_updates(&updates)?;

                Ok(CreateChatResult {
                    id: chat_id,
                    kind: "channel".to_string(),
                    name: name.to_string(),
                })
            }
            _ => {
                anyhow::bail!(
                    "Invalid chat type '{}'. Use 'group' or 'channel'.",
                    chat_type
                );
            }
        }
    }

    /// Extract channel ID from CreateChannel updates response
    fn extract_channel_id_from_updates(updates: &tl::enums::Updates) -> Result<i64> {
        match updates {
            tl::enums::Updates::Updates(u) => {
                for chat in &u.chats {
                    if let tl::enums::Chat::Channel(c) = chat {
                        return Ok(c.id);
                    }
                }
                anyhow::bail!("No channel ID found in response")
            }
            _ => anyhow::bail!("Unexpected response type from CreateChannel"),
        }
    }
}

/// Result from creating a chat
#[derive(Debug, Clone, serde::Serialize)]
pub struct CreateChatResult {
    pub id: i64,
    pub kind: String,
    pub name: String,
}

/// Extract topic_id from a raw TL message
fn extract_topic_id_from_raw(msg: &tl::enums::Message) -> Option<i32> {
    match msg {
        tl::enums::Message::Message(m) => {
            if let Some(tl::enums::MessageReplyHeader::Header(header)) = &m.reply_to {
                if header.forum_topic {
                    header.reply_to_top_id.or(header.reply_to_msg_id)
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Result from joining a chat
#[derive(Debug, Clone, serde::Serialize)]
pub struct JoinChatResult {
    pub id: i64,
    pub kind: String,
    pub name: String,
}

/// Result from creating an invite link
#[derive(Debug, Clone, serde::Serialize)]
pub struct InviteLinkResult {
    pub link: String,
    pub expire_date: Option<String>,
    pub usage_limit: Option<i32>,
}

/// Draft message info
#[derive(Debug, Clone, serde::Serialize)]
pub struct DraftInfo {
    pub chat_id: i64,
    pub text: String,
    pub date: String,
    pub reply_to_msg_id: Option<i32>,
}

/// Result from downloading media
#[derive(Debug, Clone, serde::Serialize)]
pub struct DownloadResult {
    pub path: String,
    pub media_type: String,
    pub size: u64,
}

impl App {
    /// Join a chat by invite link or username.
    pub async fn join_chat(
        &self,
        link: Option<&str>,
        username: Option<&str>,
    ) -> Result<JoinChatResult> {
        if let Some(invite_link) = link {
            // Extract hash from invite link
            let hash = extract_invite_hash(invite_link)?;

            let request = tl::functions::messages::ImportChatInvite { hash };
            let updates = self
                .tg
                .client
                .invoke(&request)
                .await
                .context("Failed to join chat via invite link")?;

            // Extract chat info from updates
            extract_chat_from_updates(&updates)
        } else if let Some(uname) = username {
            // Strip @ if present
            let clean_username = uname.trim_start_matches('@');

            // Resolve username to get the chat
            let peer = self
                .tg
                .client
                .resolve_username(clean_username)
                .await
                .context(format!("Failed to resolve username '{}'", clean_username))?;

            let peer =
                peer.ok_or_else(|| anyhow::anyhow!("Username '{}' not found", clean_username))?;

            // Join the chat
            let peer_ref = PeerRef::from(&peer);
            let input_peer: tl::enums::InputPeer = peer_ref.into();

            // Determine if it's a channel/supergroup or a basic chat
            match input_peer {
                tl::enums::InputPeer::Channel(ch) => {
                    let request = tl::functions::channels::JoinChannel {
                        channel: tl::enums::InputChannel::Channel(tl::types::InputChannel {
                            channel_id: ch.channel_id,
                            access_hash: ch.access_hash,
                        }),
                    };
                    self.tg
                        .client
                        .invoke(&request)
                        .await
                        .context("Failed to join channel")?;
                }
                _ => {
                    anyhow::bail!(
                        "Cannot join this type of chat via username. Use an invite link instead."
                    );
                }
            }

            // Return info about the joined chat
            let (kind, name) = match &peer {
                grammers_client::types::Peer::Channel(ch) => {
                    ("channel".to_string(), ch.title().to_string())
                }
                grammers_client::types::Peer::Group(g) => {
                    ("group".to_string(), g.title().unwrap_or("").to_string())
                }
                grammers_client::types::Peer::User(u) => ("user".to_string(), u.full_name()),
            };

            Ok(JoinChatResult {
                id: peer.id().bare_id(),
                kind,
                name,
            })
        } else {
            anyhow::bail!("Either link or username must be provided")
        }
    }

    /// Leave a chat.
    pub async fn leave_chat(&self, chat_id: i64) -> Result<()> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;
        let input_peer: tl::enums::InputPeer = peer_ref.into();

        match input_peer {
            tl::enums::InputPeer::Channel(ch) => {
                let request = tl::functions::channels::LeaveChannel {
                    channel: tl::enums::InputChannel::Channel(tl::types::InputChannel {
                        channel_id: ch.channel_id,
                        access_hash: ch.access_hash,
                    }),
                };
                self.tg
                    .client
                    .invoke(&request)
                    .await
                    .context(format!("Failed to leave channel {}", chat_id))?;
            }
            tl::enums::InputPeer::Chat(ch) => {
                // For basic groups, use messages.deleteChatUser
                let request = tl::functions::messages::DeleteChatUser {
                    revoke_history: false,
                    chat_id: ch.chat_id,
                    user_id: tl::enums::InputUser::UserSelf,
                };
                self.tg
                    .client
                    .invoke(&request)
                    .await
                    .context(format!("Failed to leave chat {}", chat_id))?;
            }
            _ => {
                anyhow::bail!("Cannot leave this type of chat (user chats can only be deleted)");
            }
        }

        Ok(())
    }

    /// Get the primary invite link for a chat.
    pub async fn get_invite_link(&self, chat_id: i64) -> Result<String> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;
        let input_peer: tl::enums::InputPeer = peer_ref.into();

        let request = tl::functions::messages::ExportChatInvite {
            legacy_revoke_permanent: false,
            request_needed: false,
            peer: input_peer,
            expire_date: None,
            usage_limit: None,
            title: None,
            subscription_pricing: None,
        };
        let result = self
            .tg
            .client
            .invoke(&request)
            .await
            .context(format!("Failed to get invite link for chat {}", chat_id))?;

        match result {
            tl::enums::ExportedChatInvite::ChatInviteExported(inv) => Ok(inv.link),
            tl::enums::ExportedChatInvite::ChatInvitePublicJoinRequests => {
                anyhow::bail!("Chat requires join request approval")
            }
        }
    }

    /// Create a new invite link for a chat.
    pub async fn create_invite_link(
        &self,
        chat_id: i64,
        expire_date: Option<i32>,
        usage_limit: Option<i32>,
    ) -> Result<InviteLinkResult> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;
        let input_peer: tl::enums::InputPeer = peer_ref.into();

        let request = tl::functions::messages::ExportChatInvite {
            legacy_revoke_permanent: false,
            request_needed: false,
            peer: input_peer,
            expire_date,
            usage_limit,
            title: None,
            subscription_pricing: None,
        };

        let result = self
            .tg
            .client
            .invoke(&request)
            .await
            .context(format!("Failed to create invite link for chat {}", chat_id))?;

        match result {
            tl::enums::ExportedChatInvite::ChatInviteExported(inv) => {
                let expire_str = inv.expire_date.and_then(|ts| {
                    chrono::DateTime::from_timestamp(ts as i64, 0)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                });

                Ok(InviteLinkResult {
                    link: inv.link,
                    expire_date: expire_str,
                    usage_limit: inv.usage_limit,
                })
            }
            tl::enums::ExportedChatInvite::ChatInvitePublicJoinRequests => {
                anyhow::bail!("Chat requires join request approval")
            }
        }
    }

    /// Mute notifications for a chat.
    pub async fn mute_chat(&self, chat_id: i64, mute_until: i32) -> Result<()> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;
        let input_peer: tl::enums::InputPeer = peer_ref.into();

        let settings = tl::types::InputPeerNotifySettings {
            show_previews: None,
            silent: None,
            mute_until: Some(mute_until),
            sound: None,
            stories_muted: None,
            stories_hide_sender: None,
            stories_sound: None,
        };

        let request = tl::functions::account::UpdateNotifySettings {
            peer: tl::enums::InputNotifyPeer::Peer(tl::types::InputNotifyPeer { peer: input_peer }),
            settings: tl::enums::InputPeerNotifySettings::Settings(settings),
        };

        self.tg
            .client
            .invoke(&request)
            .await
            .context(format!("Failed to mute chat {}", chat_id))?;

        Ok(())
    }

    /// Unmute notifications for a chat.
    pub async fn unmute_chat(&self, chat_id: i64) -> Result<()> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;
        let input_peer: tl::enums::InputPeer = peer_ref.into();

        let settings = tl::types::InputPeerNotifySettings {
            show_previews: None,
            silent: None,
            mute_until: Some(0), // 0 = unmute
            sound: None,
            stories_muted: None,
            stories_hide_sender: None,
            stories_sound: None,
        };

        let request = tl::functions::account::UpdateNotifySettings {
            peer: tl::enums::InputNotifyPeer::Peer(tl::types::InputNotifyPeer { peer: input_peer }),
            settings: tl::enums::InputPeerNotifySettings::Settings(settings),
        };

        self.tg
            .client
            .invoke(&request)
            .await
            .context(format!("Failed to unmute chat {}", chat_id))?;

        Ok(())
    }

    /// Download media from a message with progress indicator.
    /// Returns download result with path, media type, and size.
    pub async fn download_media(
        &self,
        chat_id: i64,
        msg_id: i64,
        output_path: Option<&str>,
    ) -> Result<DownloadResult> {
        use grammers_client::types::Downloadable;
        use std::io::Write;

        let peer_ref = self.resolve_peer_ref(chat_id).await?;

        // Fetch the specific message
        let mut message_iter = self.tg.client.iter_messages(peer_ref);
        message_iter = message_iter.offset_id(msg_id as i32 + 1).limit(1);

        let msg = message_iter
            .next()
            .await?
            .ok_or_else(|| anyhow::anyhow!("Message {} not found in chat {}", msg_id, chat_id))?;

        if msg.id() != msg_id as i32 {
            anyhow::bail!("Message {} not found in chat {}", msg_id, chat_id);
        }

        let media = msg
            .media()
            .ok_or_else(|| anyhow::anyhow!("Message {} has no media", msg_id))?;

        // Get media type
        let media_type = get_media_type(&media);

        // Determine filename and path
        let (filename, ext) = get_media_filename(&media, msg_id);

        let final_path = if let Some(out_path) = output_path {
            let p = std::path::Path::new(out_path);
            if p.is_dir() {
                p.join(format!("{}.{}", filename, ext))
            } else {
                p.to_path_buf()
            }
        } else {
            // Default to current directory
            std::path::PathBuf::from(format!("{}.{}", filename, ext))
        };

        // Get total size if available (for progress)
        let total_size = media.size();

        // Create output file
        let mut file = std::fs::File::create(&final_path)
            .context(format!("Failed to create file '{}'", final_path.display()))?;

        // Download with progress
        let mut downloaded: u64 = 0;
        let mut download_iter = self.tg.client.iter_download(&media);
        let progress_interval = std::time::Duration::from_millis(100);
        let mut last_progress = std::time::Instant::now();

        while let Some(chunk) = download_iter
            .next()
            .await
            .context("Failed to download chunk")?
        {
            file.write_all(&chunk).context("Failed to write to file")?;
            downloaded += chunk.len() as u64;

            // Show progress
            if last_progress.elapsed() >= progress_interval {
                if let Some(total) = total_size {
                    let percent = (downloaded as f64 / total as f64 * 100.0) as u32;
                    eprint!(
                        "\rDownloading... {}% ({}/{})",
                        percent,
                        format_size(downloaded),
                        format_size(total as u64)
                    );
                } else {
                    eprint!("\rDownloading... {}", format_size(downloaded));
                }
                let _ = std::io::stderr().flush();
                last_progress = std::time::Instant::now();
            }
        }

        // Clear progress line
        eprint!("\r\x1b[K");
        let _ = std::io::stderr().flush();

        Ok(DownloadResult {
            path: final_path.to_string_lossy().to_string(),
            media_type,
            size: downloaded,
        })
    }

    /// Mark messages up to a specific message ID as read.
    #[allow(dead_code)]
    pub async fn mark_read_up_to(&self, chat_id: i64, max_id: i64) -> Result<()> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;
        let input_peer: tl::enums::InputPeer = peer_ref.into();

        // Use ReadHistory for regular chats, or ReadHistory for channels
        match &input_peer {
            tl::enums::InputPeer::Channel(ch) => {
                let request = tl::functions::channels::ReadHistory {
                    channel: tl::enums::InputChannel::Channel(tl::types::InputChannel {
                        channel_id: ch.channel_id,
                        access_hash: ch.access_hash,
                    }),
                    max_id: max_id as i32,
                };
                self.tg.client.invoke(&request).await.context(format!(
                    "Failed to mark messages up to {} as read in channel {}",
                    max_id, chat_id
                ))?;
            }
            _ => {
                let request = tl::functions::messages::ReadHistory {
                    peer: input_peer,
                    max_id: max_id as i32,
                };
                self.tg.client.invoke(&request).await.context(format!(
                    "Failed to mark messages up to {} as read in chat {}",
                    max_id, chat_id
                ))?;
            }
        }
        Ok(())
    }

    /// List all drafts across chats.
    pub async fn list_drafts(&self, limit: usize) -> Result<Vec<DraftInfo>> {
        let request = tl::functions::messages::GetAllDrafts {};
        let updates = self
            .tg
            .client
            .invoke(&request)
            .await
            .context("Failed to get drafts")?;

        let mut drafts = Vec::new();

        // Extract drafts from updates
        if let tl::enums::Updates::Updates(u) = updates {
            for update in u.updates {
                if let tl::enums::Update::DraftMessage(draft_update) = update {
                    let chat_id = match draft_update.peer {
                        tl::enums::Peer::User(u) => u.user_id,
                        tl::enums::Peer::Chat(c) => c.chat_id,
                        tl::enums::Peer::Channel(c) => c.channel_id,
                    };

                    if let tl::enums::DraftMessage::Message(draft) = draft_update.draft {
                        let date = chrono::DateTime::from_timestamp(draft.date as i64, 0)
                            .map(|dt| dt.to_rfc3339())
                            .unwrap_or_else(|| "unknown".to_string());

                        let reply_to_msg_id = draft.reply_to.and_then(|r| {
                            if let tl::enums::InputReplyTo::Message(m) = r {
                                Some(m.reply_to_msg_id)
                            } else {
                                None
                            }
                        });

                        drafts.push(DraftInfo {
                            chat_id,
                            text: draft.message,
                            date,
                            reply_to_msg_id,
                        });

                        if drafts.len() >= limit {
                            break;
                        }
                    }
                }
            }
        }

        Ok(drafts)
    }

    /// Clear draft for a specific chat.
    pub async fn clear_draft(&self, chat_id: i64) -> Result<()> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;
        let input_peer: tl::enums::InputPeer = peer_ref.into();

        let request = tl::functions::messages::SaveDraft {
            no_webpage: false,
            invert_media: false,
            reply_to: None,
            peer: input_peer,
            message: String::new(), // Empty message clears the draft
            entities: None,
            media: None,
            effect: None,
            suggested_post: None,
        };

        self.tg
            .client
            .invoke(&request)
            .await
            .context(format!("Failed to clear draft for chat {}", chat_id))?;

        Ok(())
    }
}

/// Extract invite hash from various invite link formats
fn extract_invite_hash(link: &str) -> Result<String> {
    // Handle formats:
    // https://t.me/+ABC123
    // https://t.me/joinchat/ABC123
    // t.me/+ABC123
    // +ABC123 (just the hash)

    let link = link.trim();

    // If it starts with +, it's already the hash
    if let Some(hash) = link.strip_prefix('+') {
        return Ok(hash.to_string());
    }

    // Try to extract from URL
    if link.contains("t.me/+") {
        if let Some(pos) = link.find("t.me/+") {
            let hash = &link[pos + 6..];
            let hash = hash.split(['?', '/']).next().unwrap_or(hash);
            return Ok(hash.to_string());
        }
    }

    if link.contains("t.me/joinchat/") {
        if let Some(pos) = link.find("t.me/joinchat/") {
            let hash = &link[pos + 14..];
            let hash = hash.split(['?', '/']).next().unwrap_or(hash);
            return Ok(hash.to_string());
        }
    }

    anyhow::bail!(
        "Invalid invite link format. Expected: https://t.me/+HASH or https://t.me/joinchat/HASH"
    )
}

/// Extract chat info from join updates
fn extract_chat_from_updates(updates: &tl::enums::Updates) -> Result<JoinChatResult> {
    match updates {
        tl::enums::Updates::Updates(u) => {
            for chat in &u.chats {
                match chat {
                    tl::enums::Chat::Chat(c) => {
                        return Ok(JoinChatResult {
                            id: c.id,
                            kind: "group".to_string(),
                            name: c.title.clone(),
                        });
                    }
                    tl::enums::Chat::Channel(c) => {
                        let kind = if c.broadcast { "channel" } else { "supergroup" };
                        return Ok(JoinChatResult {
                            id: c.id,
                            kind: kind.to_string(),
                            name: c.title.clone(),
                        });
                    }
                    _ => {}
                }
            }
            anyhow::bail!("No chat info in join response")
        }
        _ => anyhow::bail!("Unexpected response from join"),
    }
}

/// Get filename and extension for media
fn get_media_filename(media: &grammers_client::types::Media, msg_id: i64) -> (String, String) {
    use grammers_client::types::Media;

    match media {
        Media::Photo(_) => (format!("photo_{}", msg_id), "jpg".to_string()),
        Media::Document(doc) => {
            // Try to get original filename
            if !doc.name().is_empty() {
                let name = doc.name();
                if let Some(pos) = name.rfind('.') {
                    return (name[..pos].to_string(), name[pos + 1..].to_string());
                }
                return (name.to_string(), "bin".to_string());
            }

            // Determine extension from mime type
            let ext = doc
                .mime_type()
                .map(media_mime_to_ext)
                .unwrap_or_else(|| "bin".to_string());

            let prefix = if doc.duration().is_some() {
                if doc.resolution().is_some() {
                    "video"
                } else {
                    "audio"
                }
            } else {
                "document"
            };

            (format!("{}_{}", prefix, msg_id), ext)
        }
        Media::Sticker(sticker) => {
            let ext = if sticker.is_animated() {
                "tgs".to_string()
            } else {
                sticker
                    .document
                    .mime_type()
                    .map(media_mime_to_ext)
                    .unwrap_or_else(|| "webp".to_string())
            };
            (format!("sticker_{}", msg_id), ext)
        }
        _ => (format!("media_{}", msg_id), "bin".to_string()),
    }
}

/// Convert MIME type to file extension (for media download)
fn media_mime_to_ext(mime: &str) -> String {
    match mime {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/quicktime" => "mov",
        "audio/ogg" | "audio/opus" => "ogg",
        "audio/mpeg" | "audio/mp3" => "mp3",
        "audio/mp4" | "audio/m4a" => "m4a",
        "audio/wav" => "wav",
        "audio/flac" => "flac",
        "application/pdf" => "pdf",
        "application/zip" => "zip",
        "application/x-rar-compressed" => "rar",
        "application/x-7z-compressed" => "7z",
        "text/plain" => "txt",
        "application/json" => "json",
        "application/x-tgsticker" => "tgs",
        _ => mime.split('/').next_back().unwrap_or("bin"),
    }
    .to_string()
}

/// Get media type string from Media enum
fn get_media_type(media: &grammers_client::types::Media) -> String {
    use grammers_client::types::Media;

    match media {
        Media::Photo(_) => "photo".to_string(),
        Media::Document(doc) => {
            if doc.duration().is_some() {
                if doc.resolution().is_some() {
                    "video".to_string()
                } else {
                    "audio".to_string()
                }
            } else {
                "document".to_string()
            }
        }
        Media::Sticker(_) => "sticker".to_string(),
        Media::Contact(_) => "contact".to_string(),
        Media::Poll(_) => "poll".to_string(),
        Media::Geo(_) => "geo".to_string(),
        Media::Dice(_) => "dice".to_string(),
        Media::Venue(_) => "venue".to_string(),
        Media::GeoLive(_) => "geolive".to_string(),
        Media::WebPage(_) => "webpage".to_string(),
        _ => "media".to_string(),
    }
}

/// Format file size for human readability
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
