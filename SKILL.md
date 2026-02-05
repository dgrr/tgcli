# tgcli

Telegram CLI for syncing, searching, and sending messages. Pure Rust, no TDLib.

## Setup

```bash
# Install
brew install dgrr/tgcli/tgcli
# or
cargo install tgcli

# Authenticate
tgcli auth
```

## Common Commands

### Sync

```bash
tgcli sync                    # Incremental sync (shows per-chat summary)
tgcli sync --full             # Full sync (all messages)
tgcli sync --json             # JSON output (for LLMs)
tgcli sync --quiet            # Suppress summary
tgcli sync --stream           # JSONL streaming output
tgcli sync --download-media   # Save media files
```

Default output:
```
Synced 3 chats:
  Alice               +3 messages
  Rust Developers     +12 messages
    └ General         +5 messages
    └ Off-topic       +4 messages
    └ Announcements   +3 messages
  News Channel        +1 message
```

### Chats

```bash
tgcli chats list              # List all chats
tgcli chats list --limit 20   # Limit results
tgcli chats search "name"     # Search chats
tgcli chats archive <id>      # Archive chat
tgcli chats unarchive <id>    # Unarchive chat
tgcli chats pin <id>          # Pin chat
tgcli chats mute <id>         # Mute chat
tgcli chats create --group "Name" --user <id>  # Create group
tgcli chats join <link>       # Join via invite link
tgcli chats leave <id>        # Leave chat
```

### Messages

```bash
tgcli messages list --chat <id>                    # List messages
tgcli messages list --chat <id> --topic <id>       # Forum topic messages
tgcli messages search "query"                      # Local FTS5 search
tgcli messages search --global "query"             # Telegram API search
tgcli messages show --chat <id> --message <id>     # Show single message
tgcli messages context --chat <id> --message <id>  # Show with context
tgcli messages download --chat <id> --message <id> # Download media
tgcli messages delete --chat <id> --message <id>   # Delete message
```

### Send

```bash
tgcli send --to <id> --message "Hello"             # Text message
tgcli send --to <id> --file /path/to/file          # File
tgcli send --to <id> --voice /path/to/audio.ogg    # Voice note
tgcli send --to <id> --video /path/to/video.mp4    # Video note
tgcli send --to <id> --message "Hi" --schedule "tomorrow 9am"  # Scheduled
tgcli send --to <id> --message "Hi" --reply-to <msg_id>        # Reply
```

### Contacts

```bash
tgcli contacts list           # List contacts
tgcli contacts search "name"  # Search contacts
```

### Users

```bash
tgcli users show <id>         # Show user info
tgcli users block <id>        # Block user
tgcli users unblock <id>      # Unblock user
```

### Stickers

```bash
tgcli stickers list           # List sticker packs
tgcli stickers search "query" # Search stickers
tgcli stickers send --to <id> --sticker <file_id>  # Send sticker
```

### Admin (Groups/Channels)

```bash
tgcli admin ban --chat <id> --user <id>      # Ban user
tgcli admin kick --chat <id> --user <id>     # Kick user
tgcli admin unban --chat <id> --user <id>    # Unban user
tgcli admin promote --chat <id> --user <id>  # Promote to admin
tgcli admin demote --chat <id> --user <id>   # Demote admin
```

### Folders

```bash
tgcli folders list                           # List folders
tgcli folders create "Name"                  # Create folder
tgcli folders delete <id>                    # Delete folder
```

### Other

```bash
tgcli read --chat <id>                       # Mark as read
tgcli typing --chat <id>                     # Send typing indicator
tgcli polls create --chat <id> --question "?" --option "A" --option "B"
tgcli profile show                           # Show your profile
tgcli profile set --first-name "Name"        # Update profile
tgcli completions bash                       # Generate shell completions
tgcli wipe                                   # Reset database (keeps session)
tgcli wipe --yes                             # Skip confirmation
```

## Multi-Account

```bash
tgcli --store ~/.tgcli-work sync
tgcli --store ~/.tgcli-personal chats list
```

## Output Formats

```bash
tgcli chats list              # Human-readable table
tgcli chats list --json       # JSON output
tgcli sync --stream           # JSONL streaming
```

## Storage

- `~/.tgcli/session.db` — Telegram session
- `~/.tgcli/tgcli.db` — Messages, chats, contacts (FTS5)
- `~/.tgcli/media/` — Downloaded media files
