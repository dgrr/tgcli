# tgcli

Telegram CLI tool in **pure Rust** using [grammers](https://github.com/Lonami/grammers) (MTProto). No TDLib, no C/C++ dependencies. `cargo build` and done.

## Quick Install

### Homebrew (macOS/Linux)

```bash
brew install dgrr/tgcli/tgcli
```

### Shell Script

```bash
curl -fsSL https://raw.githubusercontent.com/dgrr/tgcli/main/install.sh | bash
```

### Build from Source

```bash
cargo build --release
cp target/release/tgcli /usr/local/bin/
```

## Features

- **Auth**: Phone → code → 2FA authentication
- **Sync**: Incremental sync with checkpoints, stored in libSQL (turso) with FTS5
- **Chats**: List, search, create, join/leave, archive, pin, mute
- **Messages**: List, search (FTS5 + global API), send, edit, delete, forward, download
- **Contacts**: List and search from local DB
- **Admin**: Ban, kick, promote, demote group members
- **Read**: Mark messages as read
- **Stickers**: List, search, send stickers
- **Polls**: Create polls
- **Profile**: Show and update your profile
- **Folders**: Create and manage chat folders
- **Output**: Human-readable tables or `--json`

## Quick Start

```bash
# Authenticate
tgcli auth

# Sync messages (incremental by default)
tgcli sync

# Full sync (first time or refresh)
tgcli sync --full

# List chats
tgcli chats list

# Search messages locally (FTS5)
tgcli messages search "hello"

# Search messages globally (Telegram API)
tgcli messages search --global "hello"

# Send a message
tgcli send --to <chat_id> --message "Hello!"

# Download media from a message
tgcli messages download --chat <chat_id> --message <msg_id>
```

## Sync Behavior

- **First run**: Fetches all chats + last 50 messages per chat (configurable with `--messages-per-chat`)
- **Subsequent runs**: Pure incremental sync — only fetches new messages since last checkpoint
- **`--full`**: Forces a full sync, ignoring checkpoints

```bash
# Default incremental sync
tgcli sync

# Full sync with 100 messages per chat
tgcli sync --full --messages-per-chat 100

# Sync with progress suppressed
tgcli sync --no-progress

# Output as JSONL stream
tgcli sync --stream
```

## Daemon (Optional)

The `daemon` command is **optional** and only needed for real-time message capture.

**When to use `sync` (most use cases):**
- Periodic message fetching (cron, on-demand)
- Catching up on missed messages
- One-time data export
- CLI workflows and scripts

**When to use `daemon`:**
- Instant notifications as messages arrive
- Real-time message processing pipelines
- Live message streaming to external systems
- Continuous monitoring of specific chats

```bash
# Start daemon (listens for real-time updates)
tgcli daemon

# Daemon with JSONL output (for pipelines)
tgcli daemon --stream

# Skip background sync (pure real-time only)
tgcli daemon --no-backfill

# Ignore specific chats or all channels
tgcli daemon --ignore 123456789 --ignore-channels
```

The daemon maintains a persistent connection to Telegram and stores messages instantly as they arrive. By default, it also runs a background incremental sync to catch any messages that arrived while offline.

## Architecture

```
src/
  main.rs          CLI entry point (clap)
  cmd/             Command handlers
    auth.rs        Phone → code → 2FA
    sync.rs        Incremental/full sync
    chats.rs       List/search/create/join/leave/archive/pin/mute
    messages.rs    List/search/send/edit/delete/forward/download
    send.rs        Send text/files/voice/video
    contacts.rs    List/search contacts
    read.rs        Mark as read
    stickers.rs    List/search/send stickers
    polls.rs       Create polls
    profile.rs     Show/update profile
    folders.rs     Create/delete folders
    users.rs       Show/block/unblock users
    typing.rs      Send typing indicator
    completions.rs Shell completions
  store/           turso (libSQL) + FTS5 storage
  tg/              grammers client wrapper
  app/             App struct + business logic
  out/             Output formatting
```

## Storage

- Session: `~/.tgcli/session.db` (grammers SqliteSession)
- Data: `~/.tgcli/tgcli.db` (chats, contacts, messages + FTS5)

Multi-account support via `--store`:
```bash
tgcli --store ~/.tgcli-work sync
tgcli --store ~/.tgcli-personal sync
```

Reset local database (keeps session):
```bash
tgcli wipe        # Asks for confirmation
tgcli wipe --yes  # Skip confirmation
```

## Shell Completions

```bash
# Bash
tgcli completions bash > /etc/bash_completion.d/tgcli

# Zsh
tgcli completions zsh > ~/.zfunc/_tgcli

# Fish
tgcli completions fish > ~/.config/fish/completions/tgcli.fish
```

## Why Rust?

The Go version (`tgcli-go`) uses TDLib (C++), requiring complex cross-compilation and system dependencies. `tgcli` is pure Rust — zero C/C++ deps, single `cargo build`, tiny binary.

Uses [turso](https://github.com/tursodatabase/libsql) for database storage — a pure Rust libSQL implementation with no native compilation required.

## License

MIT
