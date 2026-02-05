//! Error handling with context wrappers for grammers errors.
//!
//! This module provides helpful error context for Telegram API operations.

use anyhow::{Context, Result};

/// Extension trait to add Telegram-specific context to errors.
pub trait TgErrorContext<T> {
    /// Add context for connection operations.
    fn context_connect(self) -> Result<T>;

    /// Add context for authorization check.
    fn context_auth_check(self) -> Result<T>;

    /// Add context for sending a login code request.
    fn context_login_code(self, phone: &str) -> Result<T>;

    /// Add context for sign-in operations.
    fn context_sign_in(self) -> Result<T>;

    /// Add context for 2FA password check.
    fn context_2fa(self) -> Result<T>;

    /// Add context for sign-out operations.
    fn context_sign_out(self) -> Result<T>;

    /// Add context for dialog iteration.
    fn context_dialogs(self) -> Result<T>;

    /// Add context for message iteration.
    fn context_messages(self, chat_id: i64) -> Result<T>;

    /// Add context for sending a message.
    fn context_send(self, chat_id: i64) -> Result<T>;

    /// Add context for sending a sticker.
    fn context_send_sticker(self, chat_id: i64) -> Result<T>;

    /// Add context for uploading a file.
    fn context_upload(self, path: &str) -> Result<T>;

    /// Add context for downloading media.
    fn context_download(self, chat_id: i64, msg_id: i32) -> Result<T>;

    /// Add context for editing a message.
    fn context_edit(self, chat_id: i64, msg_id: i64) -> Result<T>;

    /// Add context for deleting messages.
    fn context_delete(self, chat_id: i64) -> Result<T>;

    /// Add context for forwarding a message.
    fn context_forward(self, from_chat: i64, to_chat: i64) -> Result<T>;

    /// Add context for pinning/unpinning a message.
    fn context_pin(self, chat_id: i64, msg_id: i64, pin: bool) -> Result<T>;

    /// Add context for marking messages as read.
    fn context_mark_read(self, chat_id: i64) -> Result<T>;

    /// Add context for resolving a username.
    fn context_resolve_username(self, username: &str) -> Result<T>;

    /// Add context for iterating participants.
    fn context_participants(self, chat_id: i64) -> Result<T>;

    /// Add context for fetching topics.
    fn context_topics(self, chat_id: i64) -> Result<T>;

    /// Add context for folder operations.
    fn context_folder(self, chat_id: i64, folder_id: i32) -> Result<T>;

    /// Add context for fetching sticker sets.
    fn context_stickers(self) -> Result<T>;

    /// Add context for update stream errors.
    fn context_updates(self) -> Result<T>;

    /// Add context for generic API invocation.
    fn context_invoke(self, operation: &str) -> Result<T>;
}

impl<T, E: std::error::Error + Send + Sync + 'static> TgErrorContext<T>
    for std::result::Result<T, E>
{
    fn context_connect(self) -> Result<T> {
        self.context("Failed to connect to Telegram servers. Check your internet connection.")
    }

    fn context_auth_check(self) -> Result<T> {
        self.context("Failed to check authorization status")
    }

    fn context_login_code(self, phone: &str) -> Result<T> {
        self.with_context(|| {
            format!(
                "Failed to request login code for {}. Verify the phone number is correct.",
                phone
            )
        })
    }

    fn context_sign_in(self) -> Result<T> {
        self.context("Sign-in failed. The code may be incorrect or expired.")
    }

    fn context_2fa(self) -> Result<T> {
        self.context("Two-factor authentication failed. Check your password.")
    }

    fn context_sign_out(self) -> Result<T> {
        self.context("Failed to sign out from Telegram")
    }

    fn context_dialogs(self) -> Result<T> {
        self.context("Failed to fetch dialogs from Telegram")
    }

    fn context_messages(self, chat_id: i64) -> Result<T> {
        self.with_context(|| format!("Failed to fetch messages from chat {}", chat_id))
    }

    fn context_send(self, chat_id: i64) -> Result<T> {
        self.with_context(|| {
            format!(
                "Failed to send message to chat {}. The chat may not exist or you may not have permission.",
                chat_id
            )
        })
    }

    fn context_send_sticker(self, chat_id: i64) -> Result<T> {
        self.with_context(|| {
            format!(
                "Failed to send sticker to chat {}. The sticker file_id may be invalid or expired.",
                chat_id
            )
        })
    }

    fn context_upload(self, path: &str) -> Result<T> {
        self.with_context(|| format!("Failed to upload file: {}", path))
    }

    fn context_download(self, chat_id: i64, msg_id: i32) -> Result<T> {
        self.with_context(|| {
            format!(
                "Failed to download media from chat {} message {}",
                chat_id, msg_id
            )
        })
    }

    fn context_edit(self, chat_id: i64, msg_id: i64) -> Result<T> {
        self.with_context(|| {
            format!(
                "Failed to edit message {} in chat {}. You can only edit your own messages.",
                msg_id, chat_id
            )
        })
    }

    fn context_delete(self, chat_id: i64) -> Result<T> {
        self.with_context(|| {
            format!(
                "Failed to delete messages from chat {}. You may not have permission.",
                chat_id
            )
        })
    }

    fn context_forward(self, from_chat: i64, to_chat: i64) -> Result<T> {
        self.with_context(|| {
            format!(
                "Failed to forward message from chat {} to chat {}",
                from_chat, to_chat
            )
        })
    }

    fn context_pin(self, chat_id: i64, msg_id: i64, pin: bool) -> Result<T> {
        let action = if pin { "pin" } else { "unpin" };
        self.with_context(|| {
            format!(
                "Failed to {} message {} in chat {}. You may not have permission.",
                action, msg_id, chat_id
            )
        })
    }

    fn context_mark_read(self, chat_id: i64) -> Result<T> {
        self.with_context(|| format!("Failed to mark chat {} as read", chat_id))
    }

    fn context_resolve_username(self, username: &str) -> Result<T> {
        self.with_context(|| format!("Failed to resolve username @{}", username))
    }

    fn context_participants(self, chat_id: i64) -> Result<T> {
        self.with_context(|| {
            format!(
                "Failed to fetch participants from chat {}. This may require admin privileges.",
                chat_id
            )
        })
    }

    fn context_topics(self, chat_id: i64) -> Result<T> {
        self.with_context(|| format!("Failed to fetch topics for forum chat {}", chat_id))
    }

    fn context_folder(self, chat_id: i64, folder_id: i32) -> Result<T> {
        self.with_context(|| format!("Failed to move chat {} to folder {}", chat_id, folder_id))
    }

    fn context_stickers(self) -> Result<T> {
        self.context("Failed to fetch sticker sets")
    }

    fn context_updates(self) -> Result<T> {
        self.context("Error receiving updates from Telegram")
    }

    fn context_invoke(self, operation: &str) -> Result<T> {
        self.with_context(|| format!("Telegram API call failed: {}", operation))
    }
}
