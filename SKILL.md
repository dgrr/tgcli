---
name: tgcli
description: "Telegram CLI for syncing, searching, sending messages, and managing chats. Pure Rust implementation with no TDLib dependency. Supports multi-account setups, local FTS5 search, media download, scheduled messages, and real-time daemon mode. Use for interacting with Telegram from the command line or in scripts."
---

# tgcli – Telegram CLI

Pure Rust Telegram client. No TDLib. Fast. Cross-platform.

## Setup

1. Install tgcli (see [GitHub](https://github.com/dgrr/tgcli) or `cargo install tgcli`)
2. Authenticate: `tgcli auth`
3. Verify: `tgcli profile show` — confirms session is active
4. Initial sync: `tgcli sync` — fetches chats and messages
5. Verify sync: `tgcli chats list --output markdown` — confirms data is available

## Core Commands

### Sync

Fetch updates from Telegram servers. Always incremental (skips duplicates).

```bash
tgcli sync                     # Default (shows summary)
tgcli sync -q                  # Quiet (no output)
tgcli sync --full              # Full sync (all messages)
tgcli sync --download-media    # Save media files
tgcli sync --stream            # JSONL streaming (for pipelines)
```

### Chats

Manage chats: list, search, archive, pin, mute, create groups, join, leave.

```bash
tgcli chats list --output markdown           # List (markdown recommended)
tgcli chats list --limit 50                  # Limit results
tgcli chats search "DevTeam"                 # Search by name
tgcli chats archive 987654321                # Archive specific chat
tgcli chats pin 987654321                    # Pin chat
tgcli chats mute 987654321                   # Mute notifications
tgcli chats create --group "Project Alpha" --user 111222333  # Create group
tgcli chats join https://t.me/joinchat/...   # Join via invite link
tgcli chats leave 987654321                  # Leave chat (irreversible for private groups)
```

### Messages

List, search, show, download, delete messages. Supports forum topics.

```bash
tgcli messages list --chat 987654321 --output markdown  # List messages (markdown)
tgcli messages list --chat 987654321 --limit 100        # Limit to 100 messages
tgcli messages list --chat 987654321 --topic 42         # Forum topic messages
tgcli messages search "project deadline" --output markdown  # Local search (markdown)
tgcli messages search --global "urgent task"               # Telegram API search
tgcli messages show --chat 987654321 --message 4567       # Show specific message
tgcli messages context --chat 987654321 --message 4567    # Show with context
tgcli messages download --chat 987654321 --message 4567   # Download media
```

**Destructive:** Verify the target message before deleting — this cannot be undone:

```bash
tgcli messages show --chat 987654321 --message 4567       # Confirm content first
tgcli messages delete --chat 987654321 --message 4567     # Delete message
```

### Send

Send messages, files, voice/video notes, scheduled messages, replies.

```bash
tgcli send --to 123456789 --message "Hello from tgcli"      # Text message
tgcli send --to 123456789 --file report.pdf                  # Send file
tgcli send --to 123456789 --voice note.ogg                   # Voice message
tgcli send --to 123456789 --video video.mp4                  # Video note
tgcli send --to 123456789 --message "Meeting tomorrow" --schedule "tomorrow 9am"  # Scheduled
tgcli send --to 123456789 --message "Agreed" --reply-to 5678 # Reply to message
```

### Contacts & Users

```bash
tgcli contacts list --output markdown   # List contacts (markdown)
tgcli contacts search "Alice"           # Search by name
tgcli users show 123456789              # Show user profile
tgcli users block 123456789             # Block user
tgcli users unblock 123456789           # Unblock user
```

### Destructive Operations

These commands are irreversible or have significant side effects. Always verify the target first.

```bash
# Database reset — deletes all synced data (auth is preserved)
tgcli chats list --output markdown        # Review current data before wiping
tgcli wipe                                # Reset database (keeps auth)

# Admin moderation — affects real users in the group
tgcli admin ban --chat 111222333 --user 999888777    # Ban user from group
tgcli admin kick --chat 111222333 --user 999888777   # Kick user from group
```

See [REFERENCE.md](REFERENCE.md) for the full admin, stickers, folders, daemon, and utility command reference.

## Multi-Account

Use `--store` to manage multiple Telegram accounts:

```bash
tgcli --store ~/.tgcli-personal sync
tgcli --store ~/.tgcli-work chats list --output markdown
tgcli --store ~/.tgcli-bot messages list --chat 987654321
```

## Output Formats

Always use **markdown** when available (recommended for LLMs and piping):

```bash
tgcli chats list                    # Human-readable table (default)
tgcli chats list --output markdown  # Markdown (recommended for LLMs/pipes)
tgcli chats list --output json      # JSON for parsing
```

## Storage

Data stored in `--store` directory (default `~/.tgcli/`):

```
~/.tgcli/session.db    # Telegram session & authentication
~/.tgcli/tgcli.db      # Messages, chats, contacts (FTS5-indexed)
~/.tgcli/media/        # Downloaded media files
```

## Tips & Tricks

```bash
# Search messages with ripgrep
tgcli messages list --chat 987654321 --output markdown | rg "keyword"

# Export to markdown file
tgcli messages list --chat 987654321 --output markdown > exported.md

# Sync multiple accounts in parallel
for account in personal work bot; do
  tgcli --store ~/.tgcli-$account sync -q &
done
wait
```

## Links

- GitHub: https://github.com/dgrr/tgcli
- Crates.io: https://crates.io/crates/tgcli
