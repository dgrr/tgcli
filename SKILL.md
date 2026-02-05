# tgrs - Telegram CLI (Rust)

Pure Rust Telegram CLI using grammers (MTProto). No TDLib, no C/C++ dependencies.

## Installation

```bash
# Quick install
curl -fsSL https://raw.githubusercontent.com/dgrr/tgrs/main/install.sh | bash

# Or build from source
cargo build --release
```

## Authentication

```bash
tgrs auth  # Interactive: phone → code → 2FA
```

Requires Telegram API credentials (api_id, api_hash) from https://my.telegram.org

## Core Commands

### Sync
```bash
tgrs sync --once              # One-time sync
tgrs sync --follow            # Continuous sync daemon
tgrs sync --follow --socket   # With IPC socket for concurrent sends
```

### Chats
```bash
tgrs chats list                    # List all chats
tgrs chats list --query "work"     # Search chats by name
tgrs chats list --limit 50         # Limit results
tgrs chats show --id <chat_id>     # Show chat details
```

### Messages
```bash
tgrs messages list --chat <id>              # List messages in chat
tgrs messages list --chat <id> --limit 100  # With limit
tgrs messages search "keyword"              # Full-text search (FTS5)
tgrs messages search "keyword" --chat <id>  # Search in specific chat
tgrs messages show --chat <id> --id <msg>   # Show single message
tgrs messages context --chat <id> --id <msg> --before 5 --after 5  # Context
```

### Send
```bash
tgrs send --to <chat_id> --message "Hello!"
tgrs send --to <chat_id> -m "Quick message"
```

### Contacts
```bash
tgrs contacts search --query "john"
tgrs contacts show --id <user_id>
```

### Read Receipts
```bash
tgrs read --chat <chat_id> --message <msg_id>
```

## Global Flags

| Flag | Description |
|------|-------------|
| `--store <path>` | Custom data directory (default: ~/.tgrs) |
| `--json` | Output as JSON |
| `-h, --help` | Show help |
| `-V, --version` | Show version |

## Multi-Account

```bash
tgrs --store ~/.tgrs-work auth
tgrs --store ~/.tgrs-work sync --follow
tgrs --store ~/.tgrs-work chats list
```

## Storage Paths

- Session: `~/.tgrs/session.db`
- Database: `~/.tgrs/tgrs.db` (SQLite + FTS5)
- Socket: `~/.tgrs/tgrs.sock`

## Socket IPC

When sync daemon runs with `--socket`, send commands via Unix socket:

```json
{"action": "ping"}
{"action": "send_text", "to": 123456, "message": "hello"}
{"action": "mark_read", "chat": 123456, "message_ids": [789]}
```

## Common Patterns

```bash
# Initial setup
tgrs auth
tgrs sync --once

# Daily use with daemon
tgrs sync --follow --socket &
tgrs chats list
tgrs messages search "meeting"
tgrs send --to 123456 -m "On my way"

# Export chat history
tgrs messages list --chat 123456 --json > messages.json
```
