//! Plain text serializer for serde-compatible types.
//!
//! Converts any `Serialize` type to plain text tabular format:
//! - Arrays → table with header row
//! - Structs → key: value pairs

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;

/// Configuration for text output.
#[derive(Debug, Clone, Default)]
pub struct TextConfig {
    /// Columns to display (in order). If empty, auto-detect.
    pub columns: Vec<ColumnDef>,
    /// Fields to skip in output.
    pub skip_fields: Vec<String>,
}

/// Column definition for tabular output.
#[derive(Debug, Clone)]
pub struct ColumnDef {
    /// Field name in the data.
    pub field: String,
    /// Display header (defaults to humanized field name).
    pub header: Option<String>,
    /// Maximum width (truncate with ellipsis).
    pub max_width: Option<usize>,
}

#[allow(dead_code)]
impl ColumnDef {
    pub fn new(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            header: None,
            max_width: None,
        }
    }

    pub fn with_header(mut self, header: impl Into<String>) -> Self {
        self.header = Some(header.into());
        self
    }

    pub fn with_width(mut self, width: usize) -> Self {
        self.max_width = Some(width);
        self
    }
}

#[allow(dead_code)]
impl TextConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn column(mut self, col: ColumnDef) -> Self {
        self.columns.push(col);
        self
    }

    pub fn skip_field(mut self, field: impl Into<String>) -> Self {
        self.skip_fields.push(field.into());
        self
    }
}

/// Convert a serializable value to plain text.
pub fn to_text<T: Serialize>(value: &T) -> String {
    to_text_configured(value, &TextConfig::default())
}

/// Convert a serializable value to plain text with a title.
pub fn to_text_with_title<T: Serialize>(value: &T, _title: &str) -> String {
    // For text mode, we don't show title (it's typically in the command output)
    to_text_configured(value, &TextConfig::default())
}

/// Convert a serializable value to plain text with full configuration.
pub fn to_text_configured<T: Serialize>(value: &T, config: &TextConfig) -> String {
    let json = match serde_json::to_value(value) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };

    match json {
        Value::Array(arr) => format_table(&arr, config),
        Value::Object(_) => format_single(&json, config),
        _ => format_scalar(&json),
    }
}

/// Format an array as a table.
fn format_table(items: &[Value], config: &TextConfig) -> String {
    if items.is_empty() {
        return String::new();
    }

    // Determine columns from first item or config
    let columns = if config.columns.is_empty() {
        auto_detect_columns(items)
    } else {
        config.columns.clone()
    };

    if columns.is_empty() {
        return String::new();
    }

    let mut output = String::new();

    // Header row
    let headers: Vec<String> = columns
        .iter()
        .map(|c| {
            let header = c.header.clone().unwrap_or_else(|| humanize_key(&c.field));
            truncate(&header.to_uppercase(), c.max_width.unwrap_or(30))
        })
        .collect();

    // Calculate column widths
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();

    // Get all rows and update widths
    let rows: Vec<Vec<String>> = items
        .iter()
        .map(|item| {
            columns
                .iter()
                .enumerate()
                .map(|(i, col)| {
                    let val = get_field(item, &col.field);
                    let formatted = format_value_for_table(&col.field, &val);
                    let truncated = truncate(&formatted, col.max_width.unwrap_or(50));
                    widths[i] = widths[i].max(truncated.chars().count());
                    truncated
                })
                .collect()
        })
        .collect();

    // Output header
    for (i, header) in headers.iter().enumerate() {
        if i > 0 {
            output.push(' ');
        }
        output.push_str(&format!("{:<width$}", header, width = widths[i]));
    }
    output.push('\n');

    // Output rows
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            if i > 0 {
                output.push(' ');
            }
            output.push_str(&format!("{:<width$}", cell, width = widths[i]));
        }
        output.push('\n');
    }

    output
}

/// Format a single object as key: value pairs.
fn format_single(value: &Value, config: &TextConfig) -> String {
    let mut output = String::new();

    if let Value::Object(obj) = value {
        for (key, val) in obj {
            if config.skip_fields.contains(key) {
                continue;
            }
            let formatted = format_scalar(val);
            if !formatted.is_empty() {
                output.push_str(&format!("{}: {}\n", humanize_key(key), formatted));
            }
        }
    }

    output
}

/// Get a field value from an object.
fn get_field(value: &Value, field: &str) -> Value {
    match value {
        Value::Object(obj) => obj.get(field).cloned().unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

/// Format a value for table display.
fn format_value_for_table(key: &str, value: &Value) -> String {
    match value {
        Value::Null => "-".to_string(),
        Value::Bool(b) => if *b { "yes" } else { "no" }.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            // Detect ISO timestamps
            if let Ok(dt) = s.parse::<DateTime<Utc>>() {
                return dt.format("%Y-%m-%d %H:%M:%S").to_string();
            }
            // Username formatting
            if key == "username" && !s.starts_with('@') && !s.is_empty() {
                return format!("@{}", s);
            }
            s.clone()
        }
        Value::Array(arr) => format!("[{}]", arr.len()),
        Value::Object(_) => "[object]".to_string(),
    }
}

/// Format a scalar value.
fn format_scalar(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(b) => if *b { "yes" } else { "no" }.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            // Detect ISO timestamps
            if let Ok(dt) = s.parse::<DateTime<Utc>>() {
                return dt.format("%Y-%m-%d %H:%M:%S").to_string();
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

/// Auto-detect columns from array items.
fn auto_detect_columns(items: &[Value]) -> Vec<ColumnDef> {
    // Get keys from first item
    let first = match items.first() {
        Some(Value::Object(obj)) => obj,
        _ => return vec![],
    };

    // Common field order preference
    let preferred_order = [
        "kind", "type", "name", "title", "first_name", "last_name",
        "id", "user_id", "chat_id", "username", "phone",
        "text", "status", "role",
    ];

    let mut columns: Vec<ColumnDef> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();

    // Add preferred fields first (if they exist)
    for field in &preferred_order {
        if first.contains_key(*field) {
            columns.push(ColumnDef::new(*field));
            seen.insert(*field);
        }
    }

    // Add remaining fields
    for key in first.keys() {
        if !seen.contains(key.as_str()) {
            // Skip internal/complex fields
            if !key.starts_with('_')
                && !matches!(
                    key.as_str(),
                    "access_hash" | "last_sync_message_id" | "snippet" | "media_path"
                )
            {
                columns.push(ColumnDef::new(key));
            }
        }
    }

    // Apply default widths
    columns.iter_mut().for_each(|c| {
        c.max_width = Some(match c.field.as_str() {
            "name" | "title" | "text" => 30,
            "first_name" | "last_name" => 20,
            "username" => 20,
            "kind" | "type" | "status" | "role" => 12,
            "phone" => 16,
            "id" | "user_id" | "chat_id" | "sender_id" => 16,
            _ => 24,
        });
    });

    columns
}

/// Truncate a string with ellipsis.
fn truncate(s: &str, max: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max {
        s.to_string()
    } else if max > 1 {
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
    fn test_table_output() {
        let items = vec![
            TestItem { id: 1, name: "First".to_string(), active: true },
            TestItem { id: 2, name: "Second".to_string(), active: false },
        ];
        let text = to_text(&items);
        assert!(text.contains("NAME"));
        assert!(text.contains("ID"));
        assert!(text.contains("First"));
        assert!(text.contains("Second"));
    }

    #[test]
    fn test_single_item() {
        let item = TestItem {
            id: 123,
            name: "Test".to_string(),
            active: true,
        };
        let text = to_text(&item);
        assert!(text.contains("Id: 123"));
        assert!(text.contains("Name: Test"));
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 6), "hello…");
    }
}
