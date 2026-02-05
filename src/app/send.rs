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
            .upsert_chat(chat_id, "user", "", None, Some(now), false)
            .await?;

        Ok(msg.id() as i64)
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
            .upsert_chat(chat_id, "user", "", None, Some(now), false)
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
            .upsert_chat(chat_id, "user", "", None, Some(now), false)
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

    /// Forward a message from one chat to another.
    /// Returns the new message ID in the destination chat.
    pub async fn forward_message(
        &self,
        from_chat_id: i64,
        msg_id: i64,
        to_chat_id: i64,
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
            top_msg_id: None,
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

    /// Mark a chat as read.
    pub async fn mark_read(&mut self, chat_id: i64) -> Result<()> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;
        self.tg
            .client
            .mark_as_read(peer_ref)
            .await
            .context(format!("Failed to mark chat {} as read", chat_id))?;
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
            .upsert_chat(chat_id, "user", "", None, Some(now), false)
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
            .upsert_chat(chat_id, "user", "", None, Some(now), false)
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
            .upsert_chat(chat_id, "user", "", None, Some(now), false)
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
            .upsert_chat(chat_id, "user", "", None, Some(now), false)
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
            .upsert_chat(chat_id, "user", "", None, Some(now), false)
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
        anyhow::bail!("Chat {} not found. Make sure you've synced first.", chat_id);
    }

    /// Backfill (fetch older) messages for a chat.
    /// Fetches messages older than `offset_id` (going backwards in time).
    /// If `offset_id` is None, fetches from the latest messages.
    /// Returns the number of new messages fetched and stored.
    pub async fn backfill_messages(
        &self,
        chat_id: i64,
        topic_id: Option<i32>,
        offset_id: Option<i64>,
        limit: usize,
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
            .upsert_chat(chat_id, "user", "", None, Some(now), false)
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

    /// Send typing indicator to a chat.
    pub async fn set_typing(&self, chat_id: i64) -> Result<()> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;
        self.tg
            .client
            .action(peer_ref)
            .oneshot(SendMessageAction::SendMessageTypingAction)
            .await
            .context(format!(
                "Failed to set typing indicator in chat {}",
                chat_id
            ))?;
        Ok(())
    }

    /// Cancel typing indicator in a chat.
    pub async fn cancel_typing(&self, chat_id: i64) -> Result<()> {
        let peer_ref = self.resolve_peer_ref(chat_id).await?;
        self.tg
            .client
            .action(peer_ref)
            .cancel()
            .await
            .context(format!(
                "Failed to cancel typing indicator in chat {}",
                chat_id
            ))?;
        Ok(())
    }
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
