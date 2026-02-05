# tgrs

Telegram CLI tool in **pure Rust** using [grammers](https://github.com/Lonami/grammers) (MTProto). No TDLib, no C/C++ dependencies. `cargo build` and done.

## Quick Install

```bash
curl -fsSL https://raw.githubusercontent.com/dgrr/tgrs/main/install.sh | bash
```

Or build from source:

```bash
cargo build --release
cp target/release/tgrs /usr/local/bin/
```

## Features

- **Auth**: Phone → code → 2FA authentication
- **Sync**: Bootstrap + live updates, stored in SQLite with FTS5
- **Chats**: List and show chats from local DB
- **Messages**: List, search (FTS5), context view, show
- **Send**: Text messages (direct or via Unix socket IPC)
- **Contacts**: Search and show from local DB
- **Read**: Mark messages as read
- **Output**: Human-readable tables or `--json`

## Quick Start

```bash
# Build (no system dependencies needed!)
cargo build --release

# Or with just
just build
just install  # copies to /opt/homebrew/bin

# Authenticate
tgrs auth

# Sync messages
tgrs sync --once

# List chats
tgrs chats list

# Search messages
tgrs messages search "hello"

# Send a message
tgrs send --to <chat_id> --message "Hello!"
```

## Architecture

```
src/
  main.rs          CLI entry point (clap)
  cmd/             Command handlers
    auth.rs        Phone → code → 2FA
    sync.rs        Bootstrap + live sync daemon
    chats.rs       List/show chats
    messages.rs    List/search/context/show messages
    send.rs        Send text messages
    contacts.rs    Search/show contacts
    read.rs        Mark as read
    version.rs     Version info
  store/           SQLite + FTS5 storage
  tg/              grammers client wrapper
  app/             App struct + business logic
    sync.rs        Sync logic
    send.rs        Send logic
    socket.rs      Unix socket IPC
  out/             Output formatting
```

## Storage

- Session: `~/.tgrs/session.db` (grammers SqliteSession)
- Data: `~/.tgrs/tgrs.db` (chats, contacts, messages + FTS5)
- Socket: `~/.tgrs/tgrs.sock` (IPC for concurrent send during sync)

## Why Rust?

The predecessor `tgcli` uses Go + TDLib (C++), requiring complex cross-compilation and system dependencies. `tgrs` is pure Rust — zero C/C++ deps, single `cargo build`, tiny binary.

## License

MIT
