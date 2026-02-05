//! Error handling with context wrappers for grammers errors.
//!
//! This module provides helpful error context for Telegram API operations.
//! It maps Telegram API errors to user-friendly, actionable messages.

use anyhow::Result;
use grammers_mtsender::InvocationError;

/// Maps a Telegram RPC error to a user-friendly message with actionable hints.
/// Returns None if the error is not a recognized RPC error that needs special handling.
pub fn friendly_rpc_message(err: &InvocationError) -> Option<String> {
    match err {
        InvocationError::Rpc(rpc) => {
            // Flood wait - rate limiting
            if rpc.is("FLOOD_WAIT") {
                let secs = rpc.value.unwrap_or(0);
                let wait_msg = if secs >= 3600 {
                    format!("{} hour(s) {} minute(s)", secs / 3600, (secs % 3600) / 60)
                } else if secs >= 60 {
                    format!("{} minute(s) {} second(s)", secs / 60, secs % 60)
                } else {
                    format!("{} second(s)", secs)
                };
                return Some(format!(
                    "Rate limited by Telegram. Please wait {} before trying again.",
                    wait_msg
                ));
            }

            // Authentication errors
            if rpc.is("AUTH_KEY_UNREGISTERED")
                || rpc.is("SESSION_EXPIRED")
                || rpc.is("SESSION_REVOKED")
            {
                return Some(
                    "Your session has expired or been revoked. Run `tgcli auth` to re-authenticate."
                        .into(),
                );
            }
            if rpc.is("AUTH_KEY_INVALID") {
                return Some(
                    "Invalid authentication key. Delete your session and run `tgcli auth` to re-authenticate.".into(),
                );
            }
            if rpc.is("USER_DEACTIVATED") || rpc.is("USER_DEACTIVATED_BAN") {
                return Some("Your Telegram account has been deactivated or banned.".into());
            }

            // Peer/chat errors
            if rpc.is("PEER_ID_INVALID") {
                return Some("Invalid chat ID. Run `tgcli sync` to refresh your chat list.".into());
            }
            if rpc.is("CHAT_ID_INVALID") {
                return Some(
                    "Chat not found. The chat may have been deleted or you may no longer have access. Run `tgcli sync` to refresh.".into(),
                );
            }
            if rpc.is("CHANNEL_INVALID") || rpc.is("CHANNEL_PRIVATE") {
                return Some(
                    "Cannot access this channel. It may be private or you may have been removed."
                        .into(),
                );
            }
            if rpc.is("USER_NOT_PARTICIPANT") {
                return Some("You are not a member of this chat.".into());
            }
            if rpc.is("CHAT_ADMIN_REQUIRED") {
                return Some("This action requires admin privileges in the chat.".into());
            }
            if rpc.is("CHAT_RESTRICTED") {
                return Some(
                    "This chat has been restricted. You cannot perform this action.".into(),
                );
            }

            // Write/send permission errors
            if rpc.is("CHAT_WRITE_FORBIDDEN") {
                return Some("You don't have permission to send messages in this chat.".into());
            }
            if rpc.is("USER_BANNED_IN_CHANNEL") {
                return Some("You have been banned from sending messages in this channel.".into());
            }
            if rpc.name.starts_with("CHAT_SEND_") && rpc.name.ends_with("_FORBIDDEN") {
                // Handles CHAT_SEND_MEDIA_FORBIDDEN, CHAT_SEND_STICKERS_FORBIDDEN, etc.
                let media_type = rpc
                    .name
                    .strip_prefix("CHAT_SEND_")
                    .and_then(|s| s.strip_suffix("_FORBIDDEN"))
                    .map(|s| s.to_lowercase().replace('_', " "))
                    .unwrap_or_else(|| "this content".into());
                return Some(format!(
                    "You don't have permission to send {} in this chat.",
                    media_type
                ));
            }

            // Message errors
            if rpc.is("MESSAGE_ID_INVALID") {
                return Some("Message not found. It may have been deleted.".into());
            }
            if rpc.is("MESSAGE_NOT_MODIFIED") {
                return Some(
                    "Message not modified: the new content is the same as the current content."
                        .into(),
                );
            }
            if rpc.is("MESSAGE_TOO_LONG") {
                return Some(
                    "Message is too long. Telegram has a maximum message length of 4096 characters."
                        .into(),
                );
            }
            if rpc.is("MESSAGE_EMPTY") {
                return Some("Cannot send an empty message.".into());
            }
            if rpc.is("MESSAGE_EDIT_TIME_EXPIRED") {
                return Some("Cannot edit this message: the edit time window has expired.".into());
            }
            if rpc.is("MESSAGE_DELETE_FORBIDDEN") {
                return Some("You don't have permission to delete this message.".into());
            }

            // Media errors
            if rpc.is("MEDIA_EMPTY") {
                return Some("No media provided or the media is invalid.".into());
            }
            if rpc.is("PHOTO_INVALID_DIMENSIONS") {
                return Some("Photo dimensions are invalid. Try a different image.".into());
            }
            if rpc.is("FILE_REFERENCE_EXPIRED") || rpc.is("FILE_REFERENCE_INVALID") {
                return Some(
                    "Media reference has expired. Run `tgcli sync` to refresh, then try again."
                        .into(),
                );
            }
            if rpc.is("FILE_PARTS_INVALID") || rpc.name.contains("FILE_PART") {
                return Some("File upload failed. Please try again.".into());
            }

            // Username/user errors
            if rpc.is("USERNAME_INVALID") {
                return Some("Invalid username format.".into());
            }
            if rpc.is("USERNAME_NOT_OCCUPIED") {
                return Some("This username does not exist.".into());
            }
            if rpc.is("USER_ID_INVALID") {
                return Some("Invalid user ID.".into());
            }
            if rpc.is("USER_IS_BOT") {
                return Some("This action cannot be performed on a bot.".into());
            }
            if rpc.is("USER_IS_BLOCKED") {
                return Some("You have blocked this user. Unblock them first.".into());
            }
            if rpc.is("USER_PRIVACY_RESTRICTED") {
                return Some("This user's privacy settings prevent this action.".into());
            }

            // Invite/join errors
            if rpc.is("INVITE_HASH_INVALID") || rpc.is("INVITE_HASH_EXPIRED") {
                return Some("This invite link is invalid or has expired.".into());
            }
            if rpc.is("USERS_TOO_MUCH") {
                return Some("This chat has reached the maximum number of members.".into());
            }

            // Phone number errors
            if rpc.is("PHONE_NUMBER_INVALID") {
                return Some(
                    "Invalid phone number format. Use international format (e.g., +1234567890)."
                        .into(),
                );
            }
            if rpc.is("PHONE_NUMBER_BANNED") {
                return Some("This phone number has been banned from Telegram.".into());
            }
            if rpc.is("PHONE_CODE_INVALID") || rpc.is("PHONE_CODE_EXPIRED") {
                return Some(
                    "Invalid or expired verification code. Request a new code and try again."
                        .into(),
                );
            }

            // 2FA errors
            if rpc.is("PASSWORD_HASH_INVALID") || rpc.is("SRP_PASSWORD_CHANGED") {
                return Some("Incorrect 2FA password. Please try again.".into());
            }
            if rpc.is("SESSION_PASSWORD_NEEDED") {
                return Some("Two-factor authentication is required.".into());
            }

            // Timeout errors
            if rpc.is("TIMEOUT") {
                return Some("Request timed out. Please try again.".into());
            }

            // Generic server errors (5xx)
            if rpc.code >= 500 {
                return Some("Telegram server error. Please try again later.".into());
            }

            None
        }
        InvocationError::Io(_) => {
            Some("Network error. Check your internet connection and try again.".into())
        }
        InvocationError::Dropped => Some("Request was cancelled. Please try again.".into()),
        InvocationError::InvalidDc => Some(
            "Invalid datacenter. Your session may be corrupted. Try `tgcli auth` again.".into(),
        ),
        InvocationError::Authentication(_) => {
            Some("Authentication failed. Run `tgcli auth` to re-authenticate.".into())
        }
        _ => None,
    }
}

/// Extension trait to add Telegram-specific context to InvocationError results.
#[allow(dead_code)]
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

/// Helper to convert InvocationError to a user-friendly anyhow::Error with context.
#[allow(dead_code)]
fn map_invocation_error(err: InvocationError, fallback_context: &str) -> anyhow::Error {
    if let Some(friendly) = friendly_rpc_message(&err) {
        anyhow::Error::msg(friendly)
    } else {
        anyhow::Error::new(err).context(fallback_context.to_string())
    }
}

impl<T> TgErrorContext<T> for std::result::Result<T, InvocationError> {
    fn context_connect(self) -> Result<T> {
        self.map_err(|e| {
            map_invocation_error(
                e,
                "Failed to connect to Telegram. Check your internet connection and try again.",
            )
        })
    }

    fn context_auth_check(self) -> Result<T> {
        self.map_err(|e| {
            if let Some(friendly) = friendly_rpc_message(&e) {
                anyhow::Error::msg(friendly)
            } else {
                anyhow::Error::msg(
                    "Failed to check authorization. Run `tgcli auth` to authenticate.",
                )
            }
        })
    }

    fn context_login_code(self, phone: &str) -> Result<T> {
        self.map_err(|e| {
            map_invocation_error(
                e,
                &format!(
                    "Failed to request login code for {}. Verify the phone number is correct.",
                    phone
                ),
            )
        })
    }

    fn context_sign_in(self) -> Result<T> {
        self.map_err(|e| {
            map_invocation_error(
                e,
                "Sign-in failed. The code may be incorrect or expired. Request a new code.",
            )
        })
    }

    fn context_2fa(self) -> Result<T> {
        self.map_err(|e| {
            map_invocation_error(e, "Two-factor authentication failed. Check your password.")
        })
    }

    fn context_sign_out(self) -> Result<T> {
        self.map_err(|e| map_invocation_error(e, "Failed to sign out from Telegram."))
    }

    fn context_dialogs(self) -> Result<T> {
        self.map_err(|e| {
            if let Some(friendly) = friendly_rpc_message(&e) {
                anyhow::Error::msg(friendly)
            } else {
                anyhow::Error::msg(
                    "Failed to fetch chats. Check your connection and run `tgcli auth` if needed.",
                )
            }
        })
    }

    fn context_messages(self, chat_id: i64) -> Result<T> {
        self.map_err(|e| {
            if let Some(friendly) = friendly_rpc_message(&e) {
                anyhow::Error::msg(format!(
                    "Failed to fetch messages from chat {}: {}",
                    chat_id, friendly
                ))
            } else {
                anyhow::Error::msg(format!(
                    "Failed to fetch messages from chat {}. Run `tgcli sync` to refresh.",
                    chat_id
                ))
            }
        })
    }

    fn context_send(self, chat_id: i64) -> Result<T> {
        self.map_err(|e| {
            if let Some(friendly) = friendly_rpc_message(&e) {
                anyhow::Error::msg(format!(
                    "Failed to send message to chat {}: {}",
                    chat_id, friendly
                ))
            } else {
                anyhow::Error::msg(format!(
                    "Failed to send message to chat {}. The chat may not exist or you may not have permission. Run `tgcli sync` to refresh.",
                    chat_id
                ))
            }
        })
    }

    fn context_send_sticker(self, chat_id: i64) -> Result<T> {
        self.map_err(|e| {
            if let Some(friendly) = friendly_rpc_message(&e) {
                anyhow::Error::msg(format!(
                    "Failed to send sticker to chat {}: {}",
                    chat_id, friendly
                ))
            } else {
                anyhow::Error::msg(format!(
                    "Failed to send sticker to chat {}. The sticker file_id may be invalid or expired. Run `tgcli stickers list` to get fresh IDs.",
                    chat_id
                ))
            }
        })
    }

    fn context_upload(self, path: &str) -> Result<T> {
        self.map_err(|e| {
            if let Some(friendly) = friendly_rpc_message(&e) {
                anyhow::Error::msg(format!("Failed to upload '{}': {}", path, friendly))
            } else {
                anyhow::Error::msg(format!(
                    "Failed to upload '{}'. Check that the file exists and is readable.",
                    path
                ))
            }
        })
    }

    fn context_download(self, chat_id: i64, msg_id: i32) -> Result<T> {
        self.map_err(|e| {
            if let Some(friendly) = friendly_rpc_message(&e) {
                anyhow::Error::msg(format!(
                    "Failed to download media from chat {} message {}: {}",
                    chat_id, msg_id, friendly
                ))
            } else {
                anyhow::Error::msg(format!(
                    "Failed to download media from chat {} message {}. The file may no longer be available.",
                    chat_id, msg_id
                ))
            }
        })
    }

    fn context_edit(self, chat_id: i64, msg_id: i64) -> Result<T> {
        self.map_err(|e| {
            if let Some(friendly) = friendly_rpc_message(&e) {
                anyhow::Error::msg(format!(
                    "Failed to edit message {} in chat {}: {}",
                    msg_id, chat_id, friendly
                ))
            } else {
                anyhow::Error::msg(format!(
                    "Failed to edit message {} in chat {}. You can only edit your own recent messages.",
                    msg_id, chat_id
                ))
            }
        })
    }

    fn context_delete(self, chat_id: i64) -> Result<T> {
        self.map_err(|e| {
            if let Some(friendly) = friendly_rpc_message(&e) {
                anyhow::Error::msg(format!(
                    "Failed to delete messages from chat {}: {}",
                    chat_id, friendly
                ))
            } else {
                anyhow::Error::msg(format!(
                    "Failed to delete messages from chat {}. You may not have permission.",
                    chat_id
                ))
            }
        })
    }

    fn context_forward(self, from_chat: i64, to_chat: i64) -> Result<T> {
        self.map_err(|e| {
            if let Some(friendly) = friendly_rpc_message(&e) {
                anyhow::Error::msg(format!(
                    "Failed to forward from chat {} to {}: {}",
                    from_chat, to_chat, friendly
                ))
            } else {
                anyhow::Error::msg(format!(
                    "Failed to forward from chat {} to {}. Check that both chats exist and you have permission.",
                    from_chat, to_chat
                ))
            }
        })
    }

    fn context_pin(self, chat_id: i64, msg_id: i64, pin: bool) -> Result<T> {
        let action = if pin { "pin" } else { "unpin" };
        self.map_err(|e| {
            if let Some(friendly) = friendly_rpc_message(&e) {
                anyhow::Error::msg(format!(
                    "Failed to {} message {} in chat {}: {}",
                    action, msg_id, chat_id, friendly
                ))
            } else {
                anyhow::Error::msg(format!(
                    "Failed to {} message {} in chat {}. This action requires admin privileges.",
                    action, msg_id, chat_id
                ))
            }
        })
    }

    fn context_mark_read(self, chat_id: i64) -> Result<T> {
        self.map_err(|e| {
            map_invocation_error(e, &format!("Failed to mark chat {} as read.", chat_id))
        })
    }

    fn context_resolve_username(self, username: &str) -> Result<T> {
        self.map_err(|e| {
            if let Some(friendly) = friendly_rpc_message(&e) {
                anyhow::Error::msg(format!("Failed to resolve @{}: {}", username, friendly))
            } else {
                anyhow::Error::msg(format!(
                    "Failed to resolve @{}. The username may not exist or may be misspelled.",
                    username
                ))
            }
        })
    }

    fn context_participants(self, chat_id: i64) -> Result<T> {
        self.map_err(|e| {
            if let Some(friendly) = friendly_rpc_message(&e) {
                anyhow::Error::msg(format!(
                    "Failed to fetch members of chat {}: {}",
                    chat_id, friendly
                ))
            } else {
                anyhow::Error::msg(format!(
                    "Failed to fetch members of chat {}. This may require admin privileges.",
                    chat_id
                ))
            }
        })
    }

    fn context_topics(self, chat_id: i64) -> Result<T> {
        self.map_err(|e| {
            if let Some(friendly) = friendly_rpc_message(&e) {
                anyhow::Error::msg(format!(
                    "Failed to fetch topics for chat {}: {}",
                    chat_id, friendly
                ))
            } else {
                anyhow::Error::msg(format!(
                    "Failed to fetch topics for chat {}. Make sure it's a forum group.",
                    chat_id
                ))
            }
        })
    }

    fn context_folder(self, chat_id: i64, folder_id: i32) -> Result<T> {
        self.map_err(|e| {
            if let Some(friendly) = friendly_rpc_message(&e) {
                anyhow::Error::msg(format!(
                    "Failed to move chat {} to folder {}: {}",
                    chat_id, folder_id, friendly
                ))
            } else {
                anyhow::Error::msg(format!(
                    "Failed to move chat {} to folder {}.",
                    chat_id, folder_id
                ))
            }
        })
    }

    fn context_stickers(self) -> Result<T> {
        self.map_err(|e| {
            if let Some(friendly) = friendly_rpc_message(&e) {
                anyhow::Error::msg(format!("Failed to fetch sticker sets: {}", friendly))
            } else {
                anyhow::Error::msg("Failed to fetch sticker sets.")
            }
        })
    }

    fn context_updates(self) -> Result<T> {
        self.map_err(|e| {
            if let Some(friendly) = friendly_rpc_message(&e) {
                anyhow::Error::msg(format!("Error receiving updates: {}", friendly))
            } else {
                anyhow::Error::msg("Error receiving updates from Telegram. Check your connection.")
            }
        })
    }

    fn context_invoke(self, operation: &str) -> Result<T> {
        self.map_err(|e| {
            if let Some(friendly) = friendly_rpc_message(&e) {
                anyhow::Error::msg(format!("{} failed: {}", operation, friendly))
            } else {
                anyhow::Error::new(e).context(format!("{} failed", operation))
            }
        })
    }
}

// Note: TgErrorContext is only implemented for Result<T, InvocationError>.
// For other error types, use anyhow's standard .context() method.

/// Check if an InvocationError is a FLOOD_WAIT and return the wait duration.
/// Returns Some(duration) if it's a FLOOD_WAIT, None otherwise.
#[allow(dead_code)]
pub fn get_flood_wait_duration(err: &InvocationError) -> Option<std::time::Duration> {
    match err {
        InvocationError::Rpc(rpc) if rpc.is("FLOOD_WAIT") => {
            let secs = rpc.value.unwrap_or(0) as u64;
            Some(std::time::Duration::from_secs(secs))
        }
        _ => None,
    }
}

/// Retry an async operation with automatic FLOOD_WAIT handling.
///
/// If a FLOOD_WAIT error is encountered, this function will:
/// 1. Log a warning with the wait duration
/// 2. Sleep for the required duration
/// 3. Retry the operation
///
/// # Arguments
/// * `max_retries` - Maximum number of retries (0 = no retries)
/// * `operation` - Async closure that performs the operation
#[allow(dead_code)]
pub async fn with_flood_wait_retry<T, F, Fut>(
    max_retries: u32,
    operation: F,
) -> std::result::Result<T, InvocationError>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = std::result::Result<T, InvocationError>>,
{
    let mut retries = 0;
    loop {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                if let Some(wait_duration) = get_flood_wait_duration(&e) {
                    if retries < max_retries {
                        retries += 1;
                        let secs = wait_duration.as_secs();
                        if secs > 0 {
                            log::warn!(
                                "FLOOD_WAIT: Telegram rate limit hit. Waiting {} seconds before retry {}/{}...",
                                secs,
                                retries,
                                max_retries
                            );
                            eprintln!("Rate limited by Telegram. Waiting {} seconds...", secs);
                            tokio::time::sleep(wait_duration).await;
                            continue;
                        }
                    }
                }
                return Err(e);
            }
        }
    }
}
