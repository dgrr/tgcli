//! Markdown output formatting for tgcli commands.
//!
//! This module provides markdown-formatted output for list commands.
//! Format specification:
//! - Title as `# [Command Name]`
//! - Each item as `## [Primary Name/ID]`
//! - Key fields as bullet points with **bold** keys
//! - Horizontal rules (`---`) between items
//!
//! NOTE: This module is being replaced by the serde-based serializers in
//! `out/serializers/`. Most of these functions are now unused but kept
//! for backwards compatibility.

#![allow(dead_code)]

use crate::store::{Chat, Contact, Message, Topic};
use chrono::{DateTime, Utc};

/// Trait for types that can be formatted as markdown.
pub trait ToMarkdown {
    fn to_markdown(&self) -> String;
}

/// A markdown document builder for consistent formatting.
pub struct MarkdownDoc {
    lines: Vec<String>,
}

#[allow(dead_code)]
impl MarkdownDoc {
    pub fn new() -> Self {
        Self { lines: Vec::new() }
    }

    /// Add a level-1 heading (# Title)
    pub fn h1(&mut self, text: &str) -> &mut Self {
        self.lines.push(format!("# {}", text));
        self.lines.push(String::new());
        self
    }

    /// Add a level-2 heading (## Title)
    pub fn h2(&mut self, text: &str) -> &mut Self {
        self.lines.push(format!("## {}", text));
        self
    }

    /// Add a level-3 heading (### Title)
    pub fn h3(&mut self, text: &str) -> &mut Self {
        self.lines.push(format!("### {}", text));
        self
    }

    /// Add a bullet point with bold key
    pub fn field(&mut self, key: &str, value: &str) -> &mut Self {
        if !value.is_empty() {
            self.lines.push(format!("- **{}**: {}", key, value));
        }
        self
    }

    /// Add a bullet point with bold key (optional value)
    pub fn field_opt(&mut self, key: &str, value: Option<&str>) -> &mut Self {
        if let Some(v) = value {
            if !v.is_empty() {
                self.lines.push(format!("- **{}**: {}", key, v));
            }
        }
        self
    }

    /// Add a bullet point with bold key and boolean value (shows yes/no)
    pub fn field_bool(&mut self, key: &str, value: bool) -> &mut Self {
        self.lines
            .push(format!("- **{}**: {}", key, if value { "yes" } else { "no" }));
        self
    }

    /// Add a bullet point with bold key and boolean value (only if true)
    pub fn field_bool_if(&mut self, key: &str, value: bool) -> &mut Self {
        if value {
            self.lines.push(format!("- **{}**: yes", key));
        }
        self
    }

    /// Add a bullet point with bold key and numeric value
    pub fn field_num<T: std::fmt::Display>(&mut self, key: &str, value: T) -> &mut Self {
        self.lines.push(format!("- **{}**: {}", key, value));
        self
    }

    /// Add a bullet point with bold key and datetime value
    pub fn field_datetime(&mut self, key: &str, value: &DateTime<Utc>) -> &mut Self {
        self.lines
            .push(format!("- **{}**: {}", key, value.format("%Y-%m-%d %H:%M:%S UTC")));
        self
    }

    /// Add a bullet point with bold key and optional datetime value
    pub fn field_datetime_opt(&mut self, key: &str, value: Option<&DateTime<Utc>>) -> &mut Self {
        if let Some(dt) = value {
            self.field_datetime(key, dt);
        }
        self
    }

    /// Add a horizontal rule
    pub fn hr(&mut self) -> &mut Self {
        self.lines.push(String::new());
        self.lines.push("---".to_string());
        self.lines.push(String::new());
        self
    }

    /// Add a blank line
    pub fn blank(&mut self) -> &mut Self {
        self.lines.push(String::new());
        self
    }

    /// Add raw text
    pub fn text(&mut self, text: &str) -> &mut Self {
        self.lines.push(text.to_string());
        self
    }

    /// Add a code block
    pub fn code_block(&mut self, lang: &str, code: &str) -> &mut Self {
        self.lines.push(format!("```{}", lang));
        self.lines.push(code.to_string());
        self.lines.push("```".to_string());
        self
    }

    /// Add a blockquote (for message text)
    pub fn quote(&mut self, text: &str) -> &mut Self {
        for line in text.lines() {
            self.lines.push(format!("> {}", line));
        }
        self
    }

    /// Build the final markdown string
    pub fn build(&self) -> String {
        self.lines.join("\n")
    }
}

impl Default for MarkdownDoc {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Chat formatting
// ============================================================================

impl ToMarkdown for Chat {
    fn to_markdown(&self) -> String {
        let mut doc = MarkdownDoc::new();
        let display_name = if self.name.is_empty() {
            format!("Chat {}", self.id)
        } else {
            self.name.clone()
        };

        doc.h2(&display_name)
            .field_num("ID", self.id)
            .field("Kind", &self.kind)
            .field_opt("Username", self.username.as_ref().map(|u| format!("@{}", u)).as_deref())
            .field_bool_if("Forum", self.is_forum)
            .field_bool_if("Archived", self.archived)
            .field_datetime_opt("Last message", self.last_message_ts.as_ref());

        doc.build()
    }
}

/// Format a list of chats as markdown
pub fn format_chats(chats: &[Chat], title: &str) -> String {
    let mut doc = MarkdownDoc::new();
    doc.h1(title);
    doc.text(&format!("*{} chat(s)*", chats.len()));
    doc.blank();

    for (i, chat) in chats.iter().enumerate() {
        if i > 0 {
            doc.hr();
        }
        doc.text(&chat.to_markdown());
    }

    doc.build()
}

// ============================================================================
// Message formatting
// ============================================================================

impl ToMarkdown for Message {
    fn to_markdown(&self) -> String {
        let mut doc = MarkdownDoc::new();

        let sender = if self.from_me {
            "me".to_string()
        } else {
            self.sender_id.to_string()
        };

        doc.h2(&format!("Message {}", self.id))
            .field_num("Chat", self.chat_id)
            .field("From", &sender)
            .field_datetime("Time", &self.ts);

        if let Some(ref edit_ts) = self.edit_ts {
            doc.field_datetime("Edited", edit_ts);
        }

        if let Some(topic_id) = self.topic_id {
            doc.field_num("Topic", topic_id);
        }

        if let Some(reply_to) = self.reply_to_id {
            doc.field_num("Reply to", reply_to);
        }

        doc.field_opt("Media", self.media_type.as_deref());

        if !self.text.is_empty() {
            doc.blank();
            // Truncate long texts for list views
            let text = if self.text.len() > 500 {
                format!("{}…", &self.text[..500])
            } else {
                self.text.clone()
            };
            doc.quote(&text);
        }

        doc.build()
    }
}

/// Format a list of messages as markdown
pub fn format_messages(messages: &[Message], title: &str) -> String {
    let mut doc = MarkdownDoc::new();
    doc.h1(title);
    doc.text(&format!("*{} message(s)*", messages.len()));
    doc.blank();

    for (i, msg) in messages.iter().enumerate() {
        if i > 0 {
            doc.hr();
        }
        doc.text(&msg.to_markdown());
    }

    doc.build()
}

// ============================================================================
// Contact formatting
// ============================================================================

impl ToMarkdown for Contact {
    fn to_markdown(&self) -> String {
        let mut doc = MarkdownDoc::new();

        let display_name = format!("{} {}", self.first_name, self.last_name).trim().to_string();
        let display_name = if display_name.is_empty() {
            format!("User {}", self.user_id)
        } else {
            display_name
        };

        doc.h2(&display_name)
            .field_num("ID", self.user_id)
            .field("First name", &self.first_name)
            .field("Last name", &self.last_name)
            .field_opt("Username", self.username.as_ref().map(|u| format!("@{}", u)).as_deref());

        if !self.phone.is_empty() {
            doc.field("Phone", &self.phone);
        }

        doc.build()
    }
}

/// Format a list of contacts as markdown
pub fn format_contacts(contacts: &[Contact], title: &str) -> String {
    let mut doc = MarkdownDoc::new();
    doc.h1(title);
    doc.text(&format!("*{} contact(s)*", contacts.len()));
    doc.blank();

    for (i, contact) in contacts.iter().enumerate() {
        if i > 0 {
            doc.hr();
        }
        doc.text(&contact.to_markdown());
    }

    doc.build()
}

// ============================================================================
// Topic formatting
// ============================================================================

impl ToMarkdown for Topic {
    fn to_markdown(&self) -> String {
        let mut doc = MarkdownDoc::new();

        let emoji = self.icon_emoji.as_deref().unwrap_or("");
        let display_name = if emoji.is_empty() {
            self.name.clone()
        } else {
            format!("{} {}", emoji, self.name)
        };

        doc.h2(&display_name)
            .field_num("Topic ID", self.topic_id)
            .field_num("Chat ID", self.chat_id)
            .field(&"Color", &format!("#{:06X}", self.icon_color));

        if self.unread_count > 0 {
            doc.field_num("Unread", self.unread_count);
        }

        doc.build()
    }
}

/// Format a list of topics as markdown
pub fn format_topics(topics: &[Topic], chat_name: &str, chat_id: i64) -> String {
    let mut doc = MarkdownDoc::new();
    doc.h1(&format!("Topics in \"{}\"", chat_name));
    doc.text(&format!("*Chat ID: {} | {} topic(s)*", chat_id, topics.len()));
    doc.blank();

    for (i, topic) in topics.iter().enumerate() {
        if i > 0 {
            doc.hr();
        }
        doc.text(&topic.to_markdown());
    }

    doc.build()
}

// ============================================================================
// User info formatting (for users show command)
// ============================================================================

/// User information for markdown formatting
pub struct UserInfoMd {
    pub id: i64,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub username: Option<String>,
    pub phone: Option<String>,
    pub bio: Option<String>,
    pub is_bot: bool,
    pub is_verified: bool,
    pub is_premium: bool,
    pub is_scam: bool,
    pub is_fake: bool,
    pub is_blocked: bool,
    pub common_chats_count: i32,
}

impl ToMarkdown for UserInfoMd {
    fn to_markdown(&self) -> String {
        let mut doc = MarkdownDoc::new();

        let name = match (&self.first_name, &self.last_name) {
            (Some(f), Some(l)) => format!("{} {}", f, l),
            (Some(f), None) => f.clone(),
            (None, Some(l)) => l.clone(),
            (None, None) => format!("User {}", self.id),
        };

        doc.h1(&name)
            .field_num("ID", self.id);

        if let Some(ref u) = self.username {
            doc.field("Username", &format!("@{}", u));
        }

        if let Some(ref p) = self.phone {
            doc.field("Phone", &format!("+{}", p));
        }

        if let Some(ref bio) = self.bio {
            doc.blank().text("**Bio:**").quote(bio);
        }

        // Flags
        let mut flags = Vec::new();
        if self.is_bot { flags.push("🤖 Bot"); }
        if self.is_verified { flags.push("✓ Verified"); }
        if self.is_premium { flags.push("⭐ Premium"); }
        if self.is_scam { flags.push("⚠️ Scam"); }
        if self.is_fake { flags.push("⚠️ Fake"); }
        if self.is_blocked { flags.push("🚫 Blocked"); }

        if !flags.is_empty() {
            doc.blank().field("Flags", &flags.join(", "));
        }

        if self.common_chats_count > 0 {
            doc.field_num("Common chats", self.common_chats_count);
        }

        doc.build()
    }
}

// ============================================================================
// Folder formatting
// ============================================================================

/// Folder information for markdown formatting
pub struct FolderInfoMd {
    pub id: i32,
    pub title: String,
    pub emoticon: Option<String>,
    pub pinned_count: usize,
    pub include_count: usize,
    pub exclude_count: usize,
    pub contacts: bool,
    pub non_contacts: bool,
    pub groups: bool,
    pub broadcasts: bool,
    pub bots: bool,
}

impl ToMarkdown for FolderInfoMd {
    fn to_markdown(&self) -> String {
        let mut doc = MarkdownDoc::new();

        let title = if let Some(ref emoji) = self.emoticon {
            format!("{} {}", emoji, self.title)
        } else {
            self.title.clone()
        };

        doc.h2(&title)
            .field_num("ID", self.id)
            .field_num("Pinned chats", self.pinned_count)
            .field_num("Included chats", self.include_count);

        if self.exclude_count > 0 {
            doc.field_num("Excluded chats", self.exclude_count);
        }

        // Filters
        let mut filters = Vec::new();
        if self.contacts { filters.push("contacts"); }
        if self.non_contacts { filters.push("non-contacts"); }
        if self.groups { filters.push("groups"); }
        if self.broadcasts { filters.push("broadcasts"); }
        if self.bots { filters.push("bots"); }

        if !filters.is_empty() {
            doc.field("Includes", &filters.join(", "));
        }

        doc.build()
    }
}

/// Format a list of folders as markdown
pub fn format_folders(folders: &[FolderInfoMd]) -> String {
    let mut doc = MarkdownDoc::new();
    doc.h1("Chat Folders");
    doc.text(&format!("*{} folder(s)*", folders.len()));
    doc.blank();

    for (i, folder) in folders.iter().enumerate() {
        if i > 0 {
            doc.hr();
        }
        doc.text(&folder.to_markdown());
    }

    doc.build()
}

// ============================================================================
// Folder chat formatting
// ============================================================================

/// Chat in a folder for markdown formatting
pub struct FolderChatMd {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub pinned: bool,
}

impl ToMarkdown for FolderChatMd {
    fn to_markdown(&self) -> String {
        let mut doc = MarkdownDoc::new();

        let title = if self.pinned {
            format!("📌 {}", self.name)
        } else {
            self.name.clone()
        };

        doc.h2(&title)
            .field_num("ID", self.id)
            .field("Kind", &self.kind)
            .field_bool_if("Pinned", self.pinned);

        doc.build()
    }
}

/// Format folder chats as markdown
pub fn format_folder_chats(chats: &[FolderChatMd], folder_title: &str, folder_id: i32) -> String {
    let mut doc = MarkdownDoc::new();
    doc.h1(&format!("Folder: {}", folder_title));
    doc.text(&format!("*ID: {} | {} chat(s)*", folder_id, chats.len()));
    doc.blank();

    for (i, chat) in chats.iter().enumerate() {
        if i > 0 {
            doc.hr();
        }
        doc.text(&chat.to_markdown());
    }

    doc.build()
}

// ============================================================================
// Sticker pack formatting
// ============================================================================

/// Sticker pack info for markdown formatting
#[allow(dead_code)]
pub struct StickerPackMd {
    pub id: i64,
    pub short_name: String,
    pub title: String,
    pub count: i32,
    pub official: bool,
    pub emojis: bool,
}

impl ToMarkdown for StickerPackMd {
    fn to_markdown(&self) -> String {
        let mut doc = MarkdownDoc::new();

        let title = if self.official {
            format!("✓ {}", self.title)
        } else {
            self.title.clone()
        };

        doc.h2(&title)
            .field("Short name", &self.short_name)
            .field_num("Stickers", self.count)
            .field_bool_if("Official", self.official)
            .field_bool_if("Emoji pack", self.emojis);

        doc.build()
    }
}

/// Format sticker packs as markdown
pub fn format_sticker_packs(packs: &[StickerPackMd]) -> String {
    let mut doc = MarkdownDoc::new();
    doc.h1("Sticker Packs");
    doc.text(&format!("*{} pack(s) installed*", packs.len()));
    doc.blank();

    for (i, pack) in packs.iter().enumerate() {
        if i > 0 {
            doc.hr();
        }
        doc.text(&pack.to_markdown());
    }

    doc.build()
}

/// Sticker info for markdown formatting
pub struct StickerMd {
    pub emoji: String,
    pub doc_id: i64,
    pub file_id: String,
    pub animated: bool,
}

impl ToMarkdown for StickerMd {
    fn to_markdown(&self) -> String {
        let mut doc = MarkdownDoc::new();

        doc.h2(&self.emoji)
            .field_num("Document ID", self.doc_id)
            .field_bool_if("Animated", self.animated)
            .field("File ID", &self.file_id);

        doc.build()
    }
}

/// Format stickers as markdown
pub fn format_stickers(stickers: &[StickerMd], pack_name: &str, pack_title: &str) -> String {
    let mut doc = MarkdownDoc::new();
    doc.h1(&format!("Stickers: {}", pack_title));
    doc.text(&format!("*Pack: {} | {} sticker(s)*", pack_name, stickers.len()));
    doc.blank();

    for (i, sticker) in stickers.iter().enumerate() {
        if i > 0 {
            doc.hr();
        }
        doc.text(&sticker.to_markdown());
    }

    doc.build()
}

// ============================================================================
// Member formatting (for chats members)
// ============================================================================

/// Member info for markdown formatting
pub struct MemberMd {
    pub id: i64,
    pub username: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub status: String,
    pub role: String,
}

impl ToMarkdown for MemberMd {
    fn to_markdown(&self) -> String {
        let mut doc = MarkdownDoc::new();

        let name = match (&self.first_name, &self.last_name) {
            (Some(f), Some(l)) => format!("{} {}", f, l),
            (Some(f), None) => f.clone(),
            (None, Some(l)) => l.clone(),
            (None, None) => format!("User {}", self.id),
        };

        let role_icon = match self.role.as_str() {
            "creator" => "👑",
            "admin" => "⭐",
            "banned" => "🚫",
            "left" => "👋",
            _ => "",
        };

        let title = if role_icon.is_empty() {
            name
        } else {
            format!("{} {}", role_icon, name)
        };

        doc.h2(&title)
            .field_num("ID", self.id)
            .field_opt("Username", self.username.as_ref().map(|u| format!("@{}", u)).as_deref())
            .field("Role", &self.role)
            .field("Status", &self.status);

        doc.build()
    }
}

/// Format members as markdown
pub fn format_members(members: &[MemberMd], chat_name: &str, chat_id: i64) -> String {
    let mut doc = MarkdownDoc::new();
    doc.h1(&format!("Members of \"{}\"", chat_name));
    doc.text(&format!("*Chat ID: {} | {} member(s)*", chat_id, members.len()));
    doc.blank();

    for (i, member) in members.iter().enumerate() {
        if i > 0 {
            doc.hr();
        }
        doc.text(&member.to_markdown());
    }

    doc.build()
}

// ============================================================================
// Draft formatting
// ============================================================================

/// Draft info for markdown formatting
pub struct DraftMd {
    pub chat_id: i64,
    pub chat_name: Option<String>,
    pub text: String,
    pub date: String,
    pub reply_to_msg_id: Option<i32>,
}

impl ToMarkdown for DraftMd {
    fn to_markdown(&self) -> String {
        let mut doc = MarkdownDoc::new();

        let title = self.chat_name.as_deref().unwrap_or("Unknown chat");

        doc.h2(title)
            .field_num("Chat ID", self.chat_id)
            .field("Date", &self.date);

        if let Some(reply_to) = self.reply_to_msg_id {
            doc.field_num("Reply to", reply_to);
        }

        if !self.text.is_empty() {
            doc.blank().text("**Draft text:**").quote(&self.text);
        }

        doc.build()
    }
}

/// Format drafts as markdown
pub fn format_drafts(drafts: &[DraftMd]) -> String {
    let mut doc = MarkdownDoc::new();
    doc.h1("Message Drafts");
    doc.text(&format!("*{} draft(s)*", drafts.len()));
    doc.blank();

    for (i, draft) in drafts.iter().enumerate() {
        if i > 0 {
            doc.hr();
        }
        doc.text(&draft.to_markdown());
    }

    doc.build()
}

// ============================================================================
// Search results formatting
// ============================================================================

/// Search chat result for markdown formatting (used by chats search)
pub struct SearchChatResultMd {
    pub id: i64,
    pub kind: String,
    pub name: String,
    pub username: Option<String>,
}

impl ToMarkdown for SearchChatResultMd {
    fn to_markdown(&self) -> String {
        let mut doc = MarkdownDoc::new();
        let display_name = if self.name.is_empty() {
            format!("Chat {}", self.id)
        } else {
            self.name.clone()
        };

        doc.h2(&display_name)
            .field_num("ID", self.id)
            .field("Kind", &self.kind)
            .field_opt("Username", self.username.as_ref().map(|u| format!("@{}", u)).as_deref());

        doc.build()
    }
}

/// Format chat search results as markdown
pub fn format_chat_search_results(chats: &[SearchChatResultMd], query: &str) -> String {
    let mut doc = MarkdownDoc::new();
    doc.h1(&format!("Search Results for \"{}\"", query));
    doc.text(&format!("*{} result(s)*", chats.len()));
    doc.blank();

    for (i, chat) in chats.iter().enumerate() {
        if i > 0 {
            doc.hr();
        }
        doc.text(&chat.to_markdown());
    }

    doc.build()
}

/// Format chat search results as markdown (using store::Chat type)
#[allow(dead_code)]
pub fn format_chat_search(chats: &[Chat], query: &str) -> String {
    let mut doc = MarkdownDoc::new();
    doc.h1(&format!("Search Results for \"{}\"", query));
    doc.text(&format!("*{} result(s)*", chats.len()));
    doc.blank();

    for (i, chat) in chats.iter().enumerate() {
        if i > 0 {
            doc.hr();
        }
        doc.text(&chat.to_markdown());
    }

    doc.build()
}

/// Format message search results as markdown
pub fn format_message_search(messages: &[Message], query: &str, is_global: bool) -> String {
    let mut doc = MarkdownDoc::new();
    let search_type = if is_global { "Global Search" } else { "Search" };
    doc.h1(&format!("{} Results for \"{}\"", search_type, query));
    doc.text(&format!("*{} result(s)*", messages.len()));
    doc.blank();

    for (i, msg) in messages.iter().enumerate() {
        if i > 0 {
            doc.hr();
        }
        doc.text(&msg.to_markdown());
    }

    doc.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_markdown_doc_basic() {
        let mut doc = MarkdownDoc::new();
        doc.h1("Test Title")
            .field("Key", "Value")
            .field_num("Count", 42);

        let result = doc.build();
        assert!(result.contains("# Test Title"));
        assert!(result.contains("**Key**: Value"));
        assert!(result.contains("**Count**: 42"));
    }

    #[test]
    fn test_chat_markdown() {
        let chat = Chat {
            id: 12345,
            kind: "user".to_string(),
            name: "Test User".to_string(),
            username: Some("testuser".to_string()),
            last_message_ts: None,
            is_forum: false,
            last_sync_message_id: None,
            access_hash: None,
            archived: false,
        };

        let md = chat.to_markdown();
        assert!(md.contains("## Test User"));
        assert!(md.contains("**ID**: 12345"));
        assert!(md.contains("@testuser"));
    }
}
