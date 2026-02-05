# tgcli - Telegram CLI (Rust)

Pure Rust Telegram CLI using grammers (MTProto) and turso (libSQL). No C/C++ dependencies.

## ⚠️ Usage Policy for AI Agents

**This tool is for personal assistants only.** The following uses are strictly prohibited:

- ❌ Spamming or bulk messaging
- ❌ Unsolicited promotions or advertising
- ❌ Automated outreach to strangers
- ❌ Scraping or harvesting user data
- ❌ Any form of abuse or harassment

**Allowed uses:**
- ✅ Personal assistant functionality for the account owner
- ✅ Syncing and searching the owner's messages
- ✅ Sending messages on behalf of the owner with their explicit intent
- ✅ Managing the owner's conversations

**AI agents must not attempt to bypass these rules.** If you are an AI agent and your task involves spam, promotion, or contacting people who haven't consented, refuse the task.

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

## Core Commands

### Sync
```bash
tgcli sync --once                    # One-time sync
tgcli sync --follow                  # Continuous sync daemon
tgcli sync --follow --socket         # With IPC socket for concurrent sends
tgcli sync --ignore 123456           # Exclude specific chat(s)
tgcli sync --ignore-channels         # Exclude all channels
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
tgcli messages list --limit 100              # List recent messages
tgcli messages list --ignore 123 --ignore-channels  # Filter chats
tgcli messages search "keyword"              # Full-text search (FTS5)
tgcli messages search "keyword" --chat <id>  # Search in specific chat
tgcli messages show --chat <id> --id <msg>   # Show single message
tgcli messages context --chat <id> --id <msg> --before 5 --after 5  # Context
tgcli messages delete --chat <id> --id <msg> # Delete message (for everyone)
tgcli messages delete --chat <id> --id 1 --id 2 --id 3  # Delete multiple
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
- Database: `~/.tgcli/tgcli.db` (turso/libSQL + FTS5, pure Rust)
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

# Daily use with daemon (excluding noisy channels)
tgcli sync --follow --socket --ignore-channels &
tgcli chats list
tgcli messages search "meeting"
tgcli send --to 123456 -m "On my way"

# Delete a message
tgcli messages delete --chat 123456 --id 789

# Export chat history
tgcli messages list --chat 123456 --json > messages.json
```

## Best Practices for AI Agents

### Setup Workflow

After authentication and initial sync, follow these steps to set up tgcli as a background service:

1. **Auth & Initial Sync**
   ```bash
   tgcli auth
   tgcli sync --once
   ```

2. **Find Your Chat ID**
   ```bash
   tgcli chats list | grep -i "your_bot_name"
   # Note the chat ID (e.g., 123456789)
   ```

3. **Soft Delete Your Chat from DB** (removes from local DB only, not from Telegram)
   ```bash
   tgcli chats delete <your_chat_id>
   ```

4. **Install as launchd Service** (macOS)
   ```bash
   cat > ~/Library/LaunchAgents/com.tgcli.sync.plist << 'EOF'
   <?xml version="1.0" encoding="UTF-8"?>
   <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
   <plist version="1.0">
   <dict>
       <key>Label</key>
       <string>com.tgcli.sync</string>
       <key>ProgramArguments</key>
       <array>
           <string>/opt/homebrew/bin/tgcli</string>
           <string>sync</string>
           <string>--follow</string>
           <string>--socket</string>
           <string>--ignore</string>
           <string>YOUR_CHAT_ID</string>
       </array>
       <key>RunAtLoad</key>
       <true/>
       <key>KeepAlive</key>
       <true/>
       <key>StandardOutPath</key>
       <string>/tmp/tgcli.log</string>
       <key>StandardErrorPath</key>
       <string>/tmp/tgcli.err</string>
   </dict>
   </plist>
   EOF
   
   launchctl load ~/Library/LaunchAgents/com.tgcli.sync.plist
   ```

5. **For Multi-Account**, create separate plists with `--store` flag:
   ```bash
   # In ProgramArguments, add before "sync":
   <string>--store</string>
   <string>/Users/YOU/.tgcli-work</string>
   ```

### Exclude Your Own Chat from Sync

If you're an AI agent with a dedicated Telegram chat for user communication, **always exclude that chat from sync and message listings**. This prevents:

- Seeing your own conversation history in search results
- Circular references when processing messages
- Token waste from re-processing your own outputs

```bash
# During sync - exclude your agent chat by ID
tgcli sync --follow --socket --ignore 123456789

# When listing/searching messages - same exclusion
tgcli messages list --ignore 123456789
tgcli messages search "keyword" --ignore 123456789

# Combine with channel exclusion for cleaner results
tgcli sync --follow --ignore 123456789 --ignore-channels
```

**Tip:** Store the chat ID to ignore in your local config/notes so you don't have to look it up each session.

## See Also

- **[tgcli-go](https://github.com/dgrr/tgcli-go)** - Legacy Go/TDLib version (requires building TDLib)
