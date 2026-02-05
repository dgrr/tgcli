use anyhow::Result;
use clap::ValueEnum;
use serde::Serialize;
use std::fmt::Display;

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
}

impl OutputMode {
    pub fn is_json(&self) -> bool {
        matches!(self, OutputMode::Json)
    }

    pub fn is_none(&self) -> bool {
        matches!(self, OutputMode::None)
    }

    /// Write data to stdout based on output mode.
    /// - `Text`: uses Display trait
    /// - `Json`: uses Serialize trait (pretty-printed)
    /// - `None`: no output
    pub fn write<T: Display + Serialize>(&self, data: &T) {
        match self {
            OutputMode::None => {}
            OutputMode::Text => println!("{}", data),
            OutputMode::Json => {
                if let Ok(json) = serde_json::to_string_pretty(data) {
                    println!("{}", json);
                }
            }
        }
    }

    /// Write data to stderr based on output mode.
    pub fn write_err<T: Display + Serialize>(&self, data: &T) {
        match self {
            OutputMode::None => {}
            OutputMode::Text => eprintln!("{}", data),
            OutputMode::Json => {
                if let Ok(json) = serde_json::to_string_pretty(data) {
                    eprintln!("{}", json);
                }
            }
        }
    }
}

/// Write JSON to stdout.
pub fn write_json<T: Serialize>(value: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(value)?;
    println!("{}", json);
    Ok(())
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
