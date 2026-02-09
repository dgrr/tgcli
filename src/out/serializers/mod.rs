//! Custom serializers for different output formats.
//!
//! These serializers work with any `serde::Serialize` type,
//! converting them to human-readable formats.

pub mod markdown;
pub mod text;

pub use markdown::{to_markdown, to_markdown_with_title};
pub use text::{to_text, to_text_with_title};
