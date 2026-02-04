use anyhow::Result;
use serde::Serialize;

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

/// Truncate a string to the given max length with ellipsis.
pub fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else if max > 3 {
        format!("{}…", &s[..max - 1])
    } else {
        s[..max].to_string()
    }
}
