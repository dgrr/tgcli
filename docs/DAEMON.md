# tgcli daemon

The `daemon` command starts a persistent Telegram client for real-time message sync with optional HTTP RPC server.

## Usage

```bash
# Basic daemon (real-time updates + background sync)
tgcli daemon

# Daemon with RPC server
tgcli daemon --rpc

# Custom RPC port
tgcli daemon --rpc --rpc-addr 0.0.0.0:8080

# With PID file for process management
tgcli daemon --rpc --pid-file /var/run/tgcli.pid

# Quiet mode (minimal output)
tgcli daemon --rpc --quiet

# Stream updates as JSONL to stdout
tgcli daemon --stream
```

## Features

### Real-time Updates
- Immediately subscribes to Telegram updates
- Saves incoming messages to the local database as they arrive
- Handles message edits and deletions
- Updates chat metadata automatically

### Background Sync
- Catches up on missed messages when starting
- Runs concurrently with real-time updates
- Can be disabled with `--no-backfill`

### HTTP RPC Server (optional)
Enable with `--rpc`. Default address: `127.0.0.1:5556`

#### Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | `/ping` | Health check (returns `{"ok":true,"pong":true}`) |
| GET | `/status` | Server status (uptime, counts, sync state, TG connection) |
| GET | `/chats` | List chats (params: `query`, `limit`, `archived`) |
| GET | `/messages` | Get messages (params: `chat_id` required, `limit`, `after`, `before`, `topic_id`) |
| GET/POST | `/search` | Search messages (query required, optional: `chat_id`, `from_id`, `limit`, `media_type`) |
| POST | `/send` | Send a message (placeholder - not yet implemented) |
| GET | `/webhook/get` | Get webhook config (optional: `chat_id`) |
| POST | `/webhook/set` | Set webhook (`url`, `prompt`, optional: `chat_id`) |
| POST | `/webhook/remove` | Remove webhook |
| GET | `/webhook/list` | List all configured webhooks |

#### Example Requests

```bash
# Health check
curl http://localhost:5556/ping

# Server status
curl http://localhost:5556/status

# List chats
curl "http://localhost:5556/chats?limit=20"

# Get messages for a chat
curl "http://localhost:5556/messages?chat_id=123456789&limit=50"

# Search messages
curl -X POST http://localhost:5556/search \
  -H "Content-Type: application/json" \
  -d '{"query":"hello","limit":10}'

# Set webhook
curl -X POST http://localhost:5556/webhook/set \
  -H "Content-Type: application/json" \
  -d '{"url":"https://example.com/webhook","prompt":"my prompt"}'
```

## Production Deployment

### Signal Handling

- **SIGINT** (Ctrl+C): Graceful shutdown
- **SIGTERM**: Graceful shutdown (same as SIGINT)
- **SIGHUP**: Graceful shutdown (could be used for config reload in the future)

### Graceful Shutdown

1. Stops accepting new updates
2. Waits for background sync to complete (configurable timeout)
3. Syncs session state to disk
4. Cleans up PID file
5. Exits cleanly

Use `--shutdown-timeout` to configure the timeout (default: 10 seconds).

### PID File

Use `--pid-file` for process management:

```bash
tgcli daemon --rpc --pid-file /var/run/tgcli.pid

# Check if running
if [ -f /var/run/tgcli.pid ]; then
    kill -0 $(cat /var/run/tgcli.pid) 2>/dev/null && echo "Running"
fi

# Stop gracefully
kill $(cat /var/run/tgcli.pid)
```

### systemd Service

Example `/etc/systemd/system/tgcli.service`:

```ini
[Unit]
Description=Telegram CLI Daemon
After=network.target

[Service]
Type=simple
User=youruser
WorkingDirectory=/home/youruser
ExecStart=/usr/local/bin/tgcli --store /home/youruser/.tgcli daemon --rpc --quiet
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

### launchd (macOS)

Example `~/Library/LaunchAgents/com.tgcli.daemon.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.tgcli.daemon</string>
    <key>ProgramArguments</key>
    <array>
        <string>/Users/youruser/.cargo/bin/tgcli</string>
        <string>--store</string>
        <string>/Users/youruser/.tgcli</string>
        <string>daemon</string>
        <string>--rpc</string>
        <string>--quiet</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardErrorPath</key>
    <string>/Users/youruser/.tgcli/daemon.log</string>
    <key>StandardOutPath</key>
    <string>/Users/youruser/.tgcli/daemon.log</string>
</dict>
</plist>
```

Load with: `launchctl load ~/Library/LaunchAgents/com.tgcli.daemon.plist`

## Options Reference

| Option | Default | Description |
|--------|---------|-------------|
| `--no-backfill` | false | Don't run background sync |
| `--download-media` | false | Download media files for incoming messages |
| `--ignore <CHAT_ID>` | - | Chat IDs to ignore (can be repeated) |
| `--ignore-channels` | false | Skip all channel updates |
| `--quiet` | false | Suppress progress output |
| `--stream` | false | Output updates as JSONL to stdout |
| `--rpc` | false | Enable HTTP RPC server |
| `--rpc-addr` | 127.0.0.1:5556 | RPC server listen address |
| `--pid-file` | - | Write PID to this file |
| `--shutdown-timeout` | 10 | Shutdown timeout in seconds |

## Comparison with wacli

This RPC implementation is designed to be consistent with wacli's `rpc` subcommand for feature parity:

| Feature | tgcli | wacli |
|---------|-------|-------|
| Health check (`/ping`) | ✅ | ✅ |
| Status endpoint (`/status`) | ✅ | ✅ |
| List chats | ✅ | ✅ |
| Get messages | ✅ | ✅ |
| Search messages | ✅ | ✅ |
| Send messages | 🚧 (planned) | ✅ |
| Webhook management | ✅ | ✅ |
| Unix socket support | ❌ | ✅ |
| Graceful shutdown | ✅ | ✅ |
| PID file | ✅ | ❌ |
| Signal handling | ✅ | ✅ |

Note: tgcli currently supports a single webhook (not per-chat), matching the existing `tgcli hook` implementation. wacli supports multiple per-chat webhooks.
