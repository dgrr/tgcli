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
- **Sync**: Bootstrap + live updates, stored in libSQL (turso) with FTS5
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
tgcli auth

# Sync messages
tgcli sync --once

# List chats
tgcli chats list

# Search messages
tgcli messages search "hello"

# Send a message
tgcli send --to <chat_id> --message "Hello!"
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
  store/           turso (libSQL) + FTS5 storage
  tg/              grammers client wrapper
  app/             App struct + business logic
    sync.rs        Sync logic
    send.rs        Send logic
    socket.rs      Unix socket IPC
  out/             Output formatting
```

## Storage

- Session: `~/.tgcli/session.db` (grammers SqliteSession)
- Data: `~/.tgcli/tgcli.db` (chats, contacts, messages + FTS5)
- Socket: `~/.tgcli/tgcli.sock` (IPC for concurrent send during sync)

## Why Rust?

The Go version (`tgcli-go`) uses TDLib (C++), requiring complex cross-compilation and system dependencies. `tgcli` is pure Rust — zero C/C++ deps, single `cargo build`, tiny binary.

Uses [turso](https://github.com/tursodatabase/libsql) for database storage — a pure Rust libSQL implementation with no native compilation required.

## See Also

- **[tgcli-go](https://github.com/dgrr/tgcli-go)** - Go/TDLib version (more features, requires TDLib)

## License

MIT
