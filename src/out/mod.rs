pub mod markdown;
pub mod serializers;

use anyhow::Result;
use clap::ValueEnum;
use serde::Serialize;
use std::fmt::Display;

// Re-export markdown items for use in cmd modules (legacy support)
#[allow(unused_imports)]
pub use markdown::{
    format_chat_search, format_chat_search_results, format_chats, format_contacts, format_drafts,
    format_folder_chats, format_folders, format_members, format_message_search, format_messages,
    format_sticker_packs, format_stickers, format_topics, DraftMd, FolderChatMd, FolderInfoMd,
    MarkdownDoc, MemberMd, SearchChatResultMd, StickerMd, StickerPackMd, ToMarkdown, UserInfoMd,
};

// Re-export serializers for convenient access
#[allow(unused_imports)]
pub use serializers::{to_markdown, to_markdown_with_title, to_text, to_text_with_title};

/// Output mode for CLI commands
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum OutputMode {
    /// No output
    None,
    /// Human-readable text (default)
    #[default]
    Text,
    /// JSON output
    Json,
    /// Markdown output
    Markdown,
}

impl OutputMode {
    pub fn is_json(&self) -> bool {
        matches!(self, OutputMode::Json)
    }

    pub fn is_markdown(&self) -> bool {
        matches!(self, OutputMode::Markdown)
    }

    pub fn is_none(&self) -> bool {
        matches!(self, OutputMode::None)
    }

    pub fn is_text(&self) -> bool {
        matches!(self, OutputMode::Text)
    }

    /// Write data to stdout based on output mode using serde serialization.
    ///
    /// - `Text`: uses custom text serializer (tabular format)
    /// - `Json`: uses serde_json (pretty-printed)
    /// - `Markdown`: uses custom markdown serializer
    /// - `None`: no output
    pub fn write<T: Serialize>(&self, data: &T) -> Result<()> {
        match self {
            OutputMode::None => {}
            OutputMode::Text => {
                let text = serializers::to_text(data);
                if !text.is_empty() {
                    print!("{}", text);
                }
            }
            OutputMode::Markdown => {
                let md = serializers::to_markdown(data);
                if !md.is_empty() {
                    println!("{}", md);
                }
            }
            OutputMode::Json => {
                let json = serde_json::to_string_pretty(data)?;
                println!("{}", json);
            }
        }
        Ok(())
    }

    /// Write data to stdout with a title (for markdown/text headers).
    pub fn write_titled<T: Serialize>(&self, data: &T, title: &str) -> Result<()> {
        match self {
            OutputMode::None => {}
            OutputMode::Text => {
                let text = serializers::to_text_with_title(data, title);
                if !text.is_empty() {
                    print!("{}", text);
                }
            }
            OutputMode::Markdown => {
                let md = serializers::to_markdown_with_title(data, title);
                if !md.is_empty() {
                    println!("{}", md);
                }
            }
            OutputMode::Json => {
                let json = serde_json::to_string_pretty(data)?;
                println!("{}", json);
            }
        }
        Ok(())
    }

    /// Write data using legacy Display trait (for backwards compatibility).
    /// Use `write()` for new code.
    pub fn write_display<T: Display + Serialize>(&self, data: &T) {
        match self {
            OutputMode::None => {}
            OutputMode::Text | OutputMode::Markdown => println!("{}", data),
            OutputMode::Json => {
                if let Ok(json) = serde_json::to_string_pretty(data) {
                    println!("{}", json);
                }
            }
        }
    }

    /// Write data to stderr based on output mode.
    pub fn write_err<T: Serialize>(&self, data: &T) -> Result<()> {
        match self {
            OutputMode::None => {}
            OutputMode::Text => {
                let text = serializers::to_text(data);
                if !text.is_empty() {
                    eprint!("{}", text);
                }
            }
            OutputMode::Markdown => {
                let md = serializers::to_markdown(data);
                if !md.is_empty() {
                    eprintln!("{}", md);
                }
            }
            OutputMode::Json => {
                let json = serde_json::to_string_pretty(data)?;
                eprintln!("{}", json);
            }
        }
        Ok(())
    }
}

/// Write JSON to stdout.
pub fn write_json<T: Serialize>(value: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(value)?;
    println!("{}", json);
    Ok(())
}

/// Write markdown to stdout.
pub fn write_markdown(content: &str) {
    println!("{}", content);
}

/// Write an error as JSON to stderr.
#[allow(dead_code)]
pub fn write_error_json(err: &anyhow::Error) -> Result<()> {
    let json = serde_json::json!({
        "error": format!("{:#}", err),
    });
    eprintln!("{}", serde_json::to_string_pretty(&json)?);
    Ok(())
}

/// Truncate a string to the given max *character* length with ellipsis.
/// Handles multi-byte UTF-8 characters (emojis, etc.) safely.
pub fn truncate(s: &str, max: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max {
        s.to_string()
    } else if max > 1 {
        // Find the byte index of the (max-1)th character boundary
        let end_idx = s
            .char_indices()
            .nth(max - 1)
            .map(|(i, _)| i)
            .unwrap_or(s.len());
        format!("{}…", &s[..end_idx])
    } else if max == 1 {
        "…".to_string()
    } else {
        String::new()
    }
}
