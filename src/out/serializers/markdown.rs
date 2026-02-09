//! Markdown serializer for serde-compatible types.
//!
//! Converts any `Serialize` type to markdown format:
//! - Structs → fields as bullet points with **bold** keys
//! - Arrays → items separated by horizontal rules
//! - Title as `# [Title]`
//! - Item headings as `## [Name/ID]`

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;

/// Configuration for markdown output.
#[derive(Debug, Clone, Default)]
pub struct MarkdownConfig {
    /// Title for the document (appears as H1).
    pub title: Option<String>,
    /// Field name to use for item headings (H2). Falls back to "name", "title", "id".
    pub heading_field: Option<String>,
    /// Whether to show item count.
    pub show_count: bool,
    /// Fields to skip in output.
    pub skip_fields: Vec<String>,
}

#[allow(dead_code)]
impl MarkdownConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_heading_field(mut self, field: impl Into<String>) -> Self {
        self.heading_field = Some(field.into());
        self
    }

    pub fn with_count(mut self) -> Self {
        self.show_count = true;
        self
    }

    pub fn skip_field(mut self, field: impl Into<String>) -> Self {
        self.skip_fields.push(field.into());
        self
    }
}

/// Convert a serializable value to markdown.
pub fn to_markdown<T: Serialize>(value: &T) -> String {
    to_markdown_configured(value, &MarkdownConfig::default())
}

/// Convert a serializable value to markdown with a title.
pub fn to_markdown_with_title<T: Serialize>(value: &T, title: &str) -> String {
    to_markdown_configured(value, &MarkdownConfig::new().with_title(title).with_count())
}

/// Convert a serializable value to markdown with full configuration.
pub fn to_markdown_configured<T: Serialize>(value: &T, config: &MarkdownConfig) -> String {
    let json = match serde_json::to_value(value) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };

    let mut output = String::new();

    // Add title if provided
    if let Some(ref title) = config.title {
        output.push_str(&format!("# {}\n\n", title));
    }

    format_root_value(&json, &mut output, config);
    output
}

/// Format the root value (handles arrays specially for count header).
fn format_root_value(value: &Value, output: &mut String, config: &MarkdownConfig) {
    match value {
        Value::Array(arr) => {
            if config.show_count {
                output.push_str(&format!("*{} item(s)*\n\n", arr.len()));
            }
            for (i, item) in arr.iter().enumerate() {
                if i > 0 {
                    output.push_str("\n---\n\n");
                }
                format_item(item, output, config);
            }
        }
        Value::Object(_) => {
            format_item(value, output, config);
        }
        _ => {
            output.push_str(&format_scalar(value));
        }
    }
}

/// Format a single item (typically a struct serialized as object).
fn format_item(value: &Value, output: &mut String, config: &MarkdownConfig) {
    if let Value::Object(obj) = value {
        // Determine heading from configured field or common fields
        let heading = get_item_heading(obj, config);
        if let Some(h) = heading {
            output.push_str(&format!("## {}\n", h));
        }

        // Output fields as bullet points
        for (key, val) in obj {
            // Skip fields that are used for heading or in skip list
            if config.skip_fields.contains(key) {
                continue;
            }
            // Skip the heading field if it was used
            if config.heading_field.as_ref() == Some(key) {
                continue;
            }
            // Skip common heading fields that were used
            if matches!(key.as_str(), "name" | "title") && get_item_heading(obj, config).is_some() {
                // Only skip if this field was actually used for heading
                if obj.get(key).and_then(|v| v.as_str()).map(|s| !s.is_empty()).unwrap_or(false) {
                    continue;
                }
            }

            format_field(key, val, output, config);
        }
    } else {
        output.push_str(&format_scalar(value));
        output.push('\n');
    }
}

/// Get heading for an item from its fields.
fn get_item_heading(obj: &serde_json::Map<String, Value>, config: &MarkdownConfig) -> Option<String> {
    // Try configured heading field first
    if let Some(ref field) = config.heading_field {
        if let Some(val) = obj.get(field) {
            let s = format_scalar(val);
            if !s.is_empty() {
                return Some(s);
            }
        }
    }

    // Try common heading fields
    for field in &["name", "title", "first_name"] {
        if let Some(val) = obj.get(*field) {
            let s = format_scalar(val);
            if !s.is_empty() {
                // For first_name, try to combine with last_name
                if *field == "first_name" {
                    if let Some(last) = obj.get("last_name").and_then(|v| v.as_str()) {
                        if !last.is_empty() {
                            return Some(format!("{} {}", s, last));
                        }
                    }
                }
                return Some(s);
            }
        }
    }

    // Fall back to ID if nothing else
    if let Some(val) = obj.get("id").or_else(|| obj.get("user_id")).or_else(|| obj.get("chat_id")) {
        return Some(format_scalar(val));
    }

    None
}

/// Format a field as a bullet point.
fn format_field(key: &str, value: &Value, output: &mut String, _config: &MarkdownConfig) {
    match value {
        Value::Null => {}
        Value::String(s) if s.is_empty() => {}
        Value::Array(arr) if arr.is_empty() => {}
        Value::Bool(b) => {
            // Only show true booleans by default for cleaner output
            if *b {
                output.push_str(&format!("- **{}**: yes\n", humanize_key(key)));
            }
        }
        Value::Object(obj) => {
            // Nested object - format inline or as sub-list
            if obj.is_empty() {
                return;
            }
            output.push_str(&format!("- **{}**:\n", humanize_key(key)));
            for (k, v) in obj {
                if !matches!(v, Value::Null) {
                    output.push_str(&format!("  - **{}**: {}\n", humanize_key(k), format_scalar(v)));
                }
            }
        }
        Value::Array(arr) => {
            output.push_str(&format!("- **{}**: ", humanize_key(key)));
            let items: Vec<String> = arr.iter().map(format_scalar).collect();
            output.push_str(&items.join(", "));
            output.push('\n');
        }
        _ => {
            let formatted = format_scalar_with_key(key, value);
            if !formatted.is_empty() {
                output.push_str(&format!("- **{}**: {}\n", humanize_key(key), formatted));
            }
        }
    }
}

/// Format a scalar value with key context (for special formatting).
fn format_scalar_with_key(key: &str, value: &Value) -> String {
    match value {
        Value::String(s) => {
            // Detect ISO timestamps and format them nicely
            if let Ok(dt) = s.parse::<DateTime<Utc>>() {
                return dt.format("%Y-%m-%d %H:%M:%S UTC").to_string();
            }
            // Username formatting
            if key == "username" && !s.starts_with('@') && !s.is_empty() {
                return format!("@{}", s);
            }
            s.clone()
        }
        _ => format_scalar(value),
    }
}

/// Format a scalar value to string.
fn format_scalar(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(b) => if *b { "yes" } else { "no" }.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            // Detect ISO timestamps and format them nicely
            if let Ok(dt) = s.parse::<DateTime<Utc>>() {
                return dt.format("%Y-%m-%d %H:%M:%S UTC").to_string();
            }
            s.clone()
        }
        Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(format_scalar).collect();
            items.join(", ")
        }
        Value::Object(_) => "[object]".to_string(),
    }
}

/// Convert snake_case key to Title Case.
fn humanize_key(key: &str) -> String {
    key.replace('_', " ")
        .split(' ')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Serialize)]
    struct TestItem {
        id: i64,
        name: String,
        active: bool,
    }

    #[test]
    fn test_single_item() {
        let item = TestItem {
            id: 123,
            name: "Test".to_string(),
            active: true,
        };
        let md = to_markdown(&item);
        assert!(md.contains("## Test"));
        assert!(md.contains("**Id**: 123"));
        assert!(md.contains("**Active**: yes"));
    }

    #[test]
    fn test_array_with_title() {
        let items = vec![
            TestItem { id: 1, name: "First".to_string(), active: true },
            TestItem { id: 2, name: "Second".to_string(), active: false },
        ];
        let md = to_markdown_with_title(&items, "Test Items");
        assert!(md.contains("# Test Items"));
        assert!(md.contains("*2 item(s)*"));
        assert!(md.contains("## First"));
        assert!(md.contains("---"));
        assert!(md.contains("## Second"));
    }

    #[test]
    fn test_humanize_key() {
        assert_eq!(humanize_key("first_name"), "First Name");
        assert_eq!(humanize_key("id"), "Id");
        assert_eq!(humanize_key("last_message_ts"), "Last Message Ts");
    }
}
