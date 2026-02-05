# tgcli - Telegram CLI (Rust)

Pure Rust Telegram CLI using grammers (MTProto). No TDLib, no C/C++ dependencies.

## Installation

```bash
# Quick install
curl -fsSL https://raw.githubusercontent.com/dgrr/tgcli/main/install.sh | bash

# Or build from source
cargo build --release
```

## Authentication

```bash
tgcli auth  # Interactive: phone → code → 2FA
```

Requires Telegram API credentials (api_id, api_hash) from https://my.telegram.org

## Core Commands

### Sync
```bash
tgcli sync --once              # One-time sync
tgcli sync --follow            # Continuous sync daemon
tgcli sync --follow --socket   # With IPC socket for concurrent sends
```

### Chats
```bash
tgcli chats list                    # List all chats
tgcli chats list --query "work"     # Search chats by name
tgcli chats list --limit 50         # Limit results
tgcli chats show --id <chat_id>     # Show chat details
```

### Messages
```bash
tgcli messages list --chat <id>              # List messages in chat
tgcli messages list --chat <id> --limit 100  # With limit
tgcli messages search "keyword"              # Full-text search (FTS5)
tgcli messages search "keyword" --chat <id>  # Search in specific chat
tgcli messages show --chat <id> --id <msg>   # Show single message
tgcli messages context --chat <id> --id <msg> --before 5 --after 5  # Context
```

### Send
```bash
tgcli send --to <chat_id> --message "Hello!"
tgcli send --to <chat_id> -m "Quick message"
```

### Contacts
```bash
tgcli contacts search --query "john"
tgcli contacts show --id <user_id>
```

### Read Receipts
```bash
tgcli read --chat <chat_id> --message <msg_id>
```

## Global Flags

| Flag | Description |
|------|-------------|
| `--store <path>` | Custom data directory (default: ~/.tgcli) |
| `--json` | Output as JSON |
| `-h, --help` | Show help |
| `-V, --version` | Show version |

## Multi-Account

```bash
tgcli --store ~/.tgcli-work auth
tgcli --store ~/.tgcli-work sync --follow
tgcli --store ~/.tgcli-work chats list
```

## Storage Paths

- Session: `~/.tgcli/session.db`
- Database: `~/.tgcli/tgcli.db` (SQLite + FTS5)
- Socket: `~/.tgcli/tgcli.sock`

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
tgcli auth
tgcli sync --once

# Daily use with daemon
tgcli sync --follow --socket &
tgcli chats list
tgcli messages search "meeting"
tgcli send --to 123456 -m "On my way"

# Export chat history
tgcli messages list --chat 123456 --json > messages.json
```

## vs tgcli-go

| | tgcli | tgcli-go |
|---|-------|------|
| Language | Rust | Go |
| Backend | grammers (pure Rust) | TDLib (C++) |
| Dependencies | None | Requires TDLib |
| Features | Core features | More complete |
| Binary size | Larger | Smaller |
