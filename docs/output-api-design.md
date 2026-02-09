# Output API Redesign: Unified `cli.output.write(&data)` Approach

**Status**: Draft  
**Date**: 2025-02-09  
**Author**: Dario (via Claude subagent)

---

## Problem Statement

Current output handling in tgcli is scattered and repetitive. Every command has patterns like:

```rust
if cli.output.is_json() {
    out::write_json(&data)?;
} else if cli.output.is_markdown() {
    out::write_markdown(&format_chats(&chats, title));
} else {
    println!("{:<12} {:<30} ...", "KIND", "NAME");
    for c in &chats {
        println!("{:<12} {:<30}", c.kind, c.name);
    }
}
```

This leads to:
- Code duplication (~30 lines per command × 20+ commands)
- Inconsistent formatting across commands
- Easy to forget a format (e.g., adding markdown but not text)
- Hard to add new output formats (e.g., YAML, CSV)

---

## Goals

1. **Single call site**: Replace scattered if/else with `cli.output.write(&data)?`
2. **Type-safe**: Compile-time guarantee that types support all formats
3. **Extensible**: Easy to add new output formats or types
4. **Backward compatible**: Current code continues working during migration
5. **Minimal boilerplate**: Use derive macros where possible

---

## Design Overview

```
┌───────────────┐      ┌──────────────┐      ┌────────────────┐
│ OutputMode    │─────▶│  Writable    │─────▶│  Serializer    │
│ (json/md/txt) │      │  trait       │      │  (per format)  │
└───────────────┘      └──────────────┘      └────────────────┘
                              │
                              ▼
                       ┌──────────────┐
                       │ Chat, Message│
                       │ Contact, ... │
                       └──────────────┘
```

---

## New Trait Definitions

### 1. The `Writable` Trait

```rust
// src/out/writable.rs

use anyhow::Result;
use serde::Serialize;
use std::io::Write;

/// A type that can be written in multiple output formats.
/// 
/// This is the core trait for unified output handling.
/// Types implement this to enable `cli.output.write(&data)`.
pub trait Writable {
    /// Write as JSON (uses serde by default)
    fn write_json<W: Write>(&self, writer: W) -> Result<()>
    where
        Self: Serialize,
    {
        serde_json::to_writer_pretty(writer, self)?;
        Ok(())
    }

    /// Write as markdown
    fn write_markdown<W: Write>(&self, writer: W) -> Result<()>;

    /// Write as plain text (human-readable, often tabular)
    fn write_text<W: Write>(&self, writer: W) -> Result<()>;
}

/// Helper trait for types that are Serialize + Writable
/// This enables `cli.output.write(&data)` to just work.
pub trait OutputWritable: Writable + Serialize {}

// Blanket impl: anything that's Serialize + Writable is OutputWritable
impl<T: Writable + Serialize> OutputWritable for T {}
```

### 2. Context for Collections

```rust
/// Output context for collections that need headers/footers
pub struct WriteContext<'a> {
    /// Title for the output (used in markdown headers)
    pub title: Option<&'a str>,
    /// Whether to include headers (for text tables)
    pub include_header: bool,
    /// Show count/summary
    pub show_count: bool,
}

impl<'a> WriteContext<'a> {
    pub fn new() -> Self {
        Self {
            title: None,
            include_header: true,
            show_count: true,
        }
    }

    pub fn with_title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }
}

impl Default for WriteContext<'_> {
    fn default() -> Self {
        Self::new()
    }
}
```

### 3. Updated OutputMode

```rust
// src/out/mod.rs

use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum OutputMode {
    None,
    #[default]
    Text,
    Json,
    Markdown,
}

impl OutputMode {
    // Existing helper methods remain...
    pub fn is_json(&self) -> bool { ... }
    pub fn is_markdown(&self) -> bool { ... }
    
    /// NEW: Unified write method for Writable types
    pub fn write<T: OutputWritable>(&self, data: &T) -> Result<()> {
        match self {
            OutputMode::None => Ok(()),
            OutputMode::Json => {
                data.write_json(std::io::stdout())?;
                println!(); // trailing newline
                Ok(())
            }
            OutputMode::Markdown => {
                data.write_markdown(std::io::stdout())?;
                Ok(())
            }
            OutputMode::Text => {
                data.write_text(std::io::stdout())?;
                Ok(())
            }
        }
    }

    /// Write with context (for collections needing titles)
    pub fn write_with<T: WritableWithContext>(&self, data: &T, ctx: WriteContext) -> Result<()> {
        match self {
            OutputMode::None => Ok(()),
            OutputMode::Json => {
                data.write_json(std::io::stdout())?;
                println!();
                Ok(())
            }
            OutputMode::Markdown => {
                data.write_markdown_with(std::io::stdout(), &ctx)?;
                Ok(())
            }
            OutputMode::Text => {
                data.write_text_with(std::io::stdout(), &ctx)?;
                Ok(())
            }
        }
    }
}

/// Extended trait for types that need context
pub trait WritableWithContext: Writable + Serialize {
    fn write_markdown_with<W: Write>(&self, writer: W, ctx: &WriteContext) -> Result<()>;
    fn write_text_with<W: Write>(&self, writer: W, ctx: &WriteContext) -> Result<()>;
}
```

---

## Example Implementations

### Chat (Single Item)

```rust
// src/out/impls/chat.rs

use crate::out::{Writable, MarkdownDoc};
use crate::store::Chat;
use anyhow::Result;
use std::io::Write;

impl Writable for Chat {
    fn write_markdown<W: Write>(&self, mut w: W) -> Result<()> {
        let mut doc = MarkdownDoc::new();
        
        let display_name = if self.name.is_empty() {
            format!("Chat {}", self.id)
        } else {
            self.name.clone()
        };

        doc.h2(&display_name)
            .field_num("ID", self.id)
            .field("Kind", &self.kind)
            .field_opt("Username", self.username.as_ref().map(|u| format!("@{u}")).as_deref())
            .field_bool_if("Forum", self.is_forum)
            .field_bool_if("Archived", self.archived)
            .field_datetime_opt("Last message", self.last_message_ts.as_ref());

        writeln!(w, "{}", doc.build())?;
        Ok(())
    }

    fn write_text<W: Write>(&self, mut w: W) -> Result<()> {
        writeln!(w, "ID: {}", self.id)?;
        writeln!(w, "Kind: {}", self.kind)?;
        writeln!(w, "Name: {}", self.name)?;
        if let Some(u) = &self.username {
            writeln!(w, "Username: @{}", u)?;
        }
        if self.is_forum {
            writeln!(w, "Forum: yes")?;
        }
        if self.archived {
            writeln!(w, "Archived: yes")?;
        }
        if let Some(ts) = self.last_message_ts {
            writeln!(w, "Last message: {}", ts.to_rfc3339())?;
        }
        Ok(())
    }
}
```

### Vec<Chat> (Collection with Context)

```rust
// src/out/impls/chat.rs (continued)

impl WritableWithContext for Vec<Chat> {
    fn write_markdown_with<W: Write>(&self, mut w: W, ctx: &WriteContext) -> Result<()> {
        let mut doc = MarkdownDoc::new();
        
        let title = ctx.title.unwrap_or("Chats");
        doc.h1(title);
        
        if ctx.show_count {
            doc.text(&format!("*{} chat(s)*", self.len()));
            doc.blank();
        }

        for (i, chat) in self.iter().enumerate() {
            if i > 0 {
                doc.hr();
            }
            doc.text(&chat.to_markdown());
        }

        writeln!(w, "{}", doc.build())?;
        Ok(())
    }

    fn write_text_with<W: Write>(&self, mut w: W, ctx: &WriteContext) -> Result<()> {
        if ctx.include_header {
            writeln!(w, "{:<12} {:<30} {:<16} {:<8} LAST MESSAGE",
                "KIND", "NAME", "ID", "ARCH")?;
        }
        
        for c in self {
            let name = crate::out::truncate(&c.name, 28);
            let ts = c.last_message_ts
                .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_default();
            let kind_display = if c.is_forum {
                format!("{}[forum]", c.kind)
            } else {
                c.kind.clone()
            };
            let archived_display = if c.archived { "yes" } else { "" };
            
            writeln!(w, "{:<12} {:<30} {:<16} {:<8} {}",
                kind_display, name, c.id, archived_display, ts)?;
        }
        Ok(())
    }
}
```

### Message (Single Item)

```rust
// src/out/impls/message.rs

use crate::out::Writable;
use crate::store::Message;
use anyhow::Result;
use std::io::Write;

impl Writable for Message {
    fn write_markdown<W: Write>(&self, mut w: W) -> Result<()> {
        let mut doc = MarkdownDoc::new();
        
        let sender = if self.from_me { "me".to_string() } else { self.sender_id.to_string() };
        
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
            let text = if self.text.len() > 500 {
                format!("{}…", &self.text[..500])
            } else {
                self.text.clone()
            };
            doc.quote(&text);
        }

        writeln!(w, "{}", doc.build())?;
        Ok(())
    }

    fn write_text<W: Write>(&self, mut w: W) -> Result<()> {
        let from = if self.from_me { "me".to_string() } else { self.sender_id.to_string() };
        let ts = self.ts.format("%Y-%m-%d %H:%M:%S").to_string();
        let text = crate::out::truncate(&self.text, 70);
        
        writeln!(w, "[{}] chat:{} from:{} id:{}", ts, self.chat_id, from, self.id)?;
        if !text.is_empty() {
            writeln!(w, "  {}", text)?;
        }
        Ok(())
    }
}
```

---

## Before/After Code Examples

### Before (chats list command)

```rust
// 25 lines of output handling
let chats = store.list_chats(query.as_deref(), limit, archived_filter).await?;

if cli.output.is_json() {
    out::write_json(&chats)?;
} else if cli.output.is_markdown() {
    let title = if *archived {
        "Archived Chats"
    } else if *active {
        "Active Chats"
    } else {
        "Chats"
    };
    out::write_markdown(&format_chats(&chats, title));
} else {
    println!("{:<12} {:<30} {:<16} {:<8} LAST MESSAGE", "KIND", "NAME", "ID", "ARCH");
    for c in &chats {
        let name = out::truncate(&c.name, 28);
        let ts = c.last_message_ts
            .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_default();
        let kind_display = if c.is_forum {
            format!("{}[forum]", c.kind)
        } else {
            c.kind.clone()
        };
        let archived_display = if c.archived { "yes" } else { "" };
        println!("{:<12} {:<30} {:<16} {:<8} {}", kind_display, name, c.id, archived_display, ts);
    }
}
```

### After (chats list command)

```rust
// 4 lines of output handling
let chats = store.list_chats(query.as_deref(), limit, archived_filter).await?;

let title = match (*archived, *active) {
    (true, _) => "Archived Chats",
    (_, true) => "Active Chats", 
    _ => "Chats",
};

cli.output.write_with(&chats, WriteContext::new().with_title(title))?;
```

### Before (contacts show command)

```rust
let contact = store.get_contact(*id).await?;
match contact {
    Some(c) => {
        if cli.output.is_json() {
            out::write_json(&c)?;
        } else if cli.output.is_markdown() {
            out::write_markdown(&c.to_markdown());
        } else {
            println!("ID: {}", c.user_id);
            println!("Name: {} {}", c.first_name, c.last_name);
            if let Some(u) = &c.username {
                println!("Username: @{}", u);
            }
            if !c.phone.is_empty() {
                println!("Phone: {}", c.phone);
            }
        }
    }
    None => anyhow::bail!("Contact not found"),
}
```

### After (contacts show command)

```rust
let contact = store.get_contact(*id).await?
    .ok_or_else(|| anyhow::anyhow!("Contact not found"))?;

cli.output.write(&contact)?;
```

---

## Type Support Matrix

| Type | Single | Collection | Notes |
|------|--------|------------|-------|
| `Chat` | ✓ | `Vec<Chat>` | Needs title context |
| `Message` | ✓ | `Vec<Message>` | Needs title context |
| `Contact` | ✓ | `Vec<Contact>` | Needs title context |
| `Topic` | ✓ | `Vec<Topic>` | Needs chat name context |
| `Sticker` | ✓ | `Vec<Sticker>` | Needs pack context |
| `MemberMd` | ✓ | `Vec<MemberMd>` | Needs chat context |
| `FolderInfoMd` | ✓ | `Vec<FolderInfoMd>` | - |
| `DraftMd` | ✓ | `Vec<DraftMd>` | - |
| `UserInfoMd` | ✓ | - | Single user only |

---

## File Structure

```
src/out/
├── mod.rs              # OutputMode, re-exports
├── writable.rs         # Writable trait, WriteContext
├── markdown.rs         # MarkdownDoc builder (existing)
├── impls/
│   ├── mod.rs          # Re-exports all impls
│   ├── chat.rs         # Chat, Vec<Chat>
│   ├── message.rs      # Message, Vec<Message>
│   ├── contact.rs      # Contact, Vec<Contact>
│   ├── topic.rs        # Topic, Vec<Topic>
│   ├── sticker.rs      # Sticker types
│   ├── folder.rs       # Folder types
│   ├── member.rs       # Member types
│   └── draft.rs        # Draft types
└── text.rs             # Text formatting helpers (truncate, etc.)
```

---

## Migration Path

### Phase 1: Add Infrastructure (non-breaking)
1. Add `Writable` trait and `WriteContext`
2. Add `OutputMode::write()` and `write_with()` methods
3. Implement `Writable` for all store types
4. Keep existing `is_json()`, `is_markdown()`, etc.

### Phase 2: Migrate Commands (gradual)
1. Start with simple commands (contacts, topics, drafts)
2. Update one command at a time
3. Each migration is a small, reviewable PR
4. Existing patterns continue to work

### Phase 3: Remove Old Patterns
1. Once all commands migrated, deprecate `is_*()` methods
2. Remove `write_json()`, `write_markdown()` functions
3. Remove `format_*()` functions from markdown.rs

### Example Migration Order
```
1. contacts list/show    (simple, single type)
2. topics list           (needs context)
3. drafts list           (simple collection)
4. stickers list/show    (nested context)
5. chats list/show       (most used, test thoroughly)
6. messages list/search  (complex, streaming variant)
7. folders list/chats    (multiple related types)
```

---

## Challenges and Trade-offs

### 1. Context Complexity
**Problem**: Collections often need metadata (title, chat name, etc.)

**Solution**: `WriteContext` struct with optional fields. Types that need context implement `WritableWithContext`.

**Trade-off**: Two methods (`write` vs `write_with`) instead of one.

### 2. Text Table Formatting
**Problem**: Text output often uses aligned columns, but column widths are hardcoded.

**Solution A** (simple): Keep hardcoded widths, document them.  
**Solution B** (complex): Add column configuration to WriteContext.

**Recommendation**: Solution A for now; column widths rarely change.

### 3. Streaming Output (JSONL)
**Problem**: `messages list --stream` writes one JSON object per line.

**Solution**: Add `OutputMode::write_streaming()` or handle in command:
```rust
if cmd.stream {
    for msg in &messages {
        // JSONL bypasses Writable
        println!("{}", serde_json::to_string(msg)?);
    }
} else {
    cli.output.write_with(&messages, ctx)?;
}
```

### 4. Dynamic Titles
**Problem**: Some titles include counts or query terms.

**Solution**: `WriteContext` already supports this via `.with_title()`.

### 5. Derive Macro (Future)
**Problem**: Implementing Writable for every type is boilerplate.

**Future Solution**: `#[derive(Writable)]` macro that generates impls from struct definition + attributes:
```rust
#[derive(Writable)]
#[writable(md_heading = "name")]
pub struct Chat {
    #[writable(label = "ID")]
    pub id: i64,
    // ...
}
```

**For Now**: Manual implementations are acceptable given ~10 types.

---

## Compatibility Notes

### Existing Code Still Works
```rust
// This continues to work during migration
if cli.output.is_json() {
    out::write_json(&data)?;
} else if cli.output.is_markdown() {
    out::write_markdown(&formatted);
} else {
    // ...
}
```

### Can Mix Old and New
```rust
// New style for most cases
cli.output.write(&chat)?;

// Old style for special cases
if cli.output.is_json() {
    // custom JSON structure
    out::write_json(&serde_json::json!({ "special": true }))?;
}
```

---

## Summary

This design provides:
- ✅ Single call site: `cli.output.write(&data)`
- ✅ Type-safe: Compile-time trait bounds
- ✅ Extensible: Add new formats by adding trait methods
- ✅ Backward compatible: Old patterns still work
- ✅ Clear migration path: Gradual, one command at a time

Lines of code reduction estimate:
- ~600 lines removed (duplicate if/else chains)
- ~400 lines added (trait impls, infrastructure)
- **Net reduction: ~200 lines** + significantly improved maintainability
