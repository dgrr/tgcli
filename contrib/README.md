# Service Files

Run tgcli as a background daemon that starts automatically on login.

## macOS (launchd)

```bash
# Copy the plist to LaunchAgents
cp contrib/macos/com.tgcli.sync.plist ~/Library/LaunchAgents/

# Edit the file to customize:
# - Path to tgcli binary (if not /opt/homebrew/bin/tgcli)
# - --ignore flags to exclude specific chats
# - --ignore-channels to skip all channels

# Load and start the service
launchctl load ~/Library/LaunchAgents/com.tgcli.sync.plist

# Check status
launchctl list | grep tgcli

# View logs
tail -f /tmp/tgcli.log
tail -f /tmp/tgcli.err

# Stop and unload
launchctl unload ~/Library/LaunchAgents/com.tgcli.sync.plist
```

### Multi-Account (macOS)

For multiple Telegram accounts, create separate plists:

```bash
cp contrib/macos/com.tgcli.sync.work.plist ~/Library/LaunchAgents/

# Edit to set your username and store path
# Then load it
launchctl load ~/Library/LaunchAgents/com.tgcli.sync.work.plist
```

## Linux (systemd)

```bash
# Copy to user systemd directory
mkdir -p ~/.config/systemd/user
cp contrib/linux/tgcli-sync.service ~/.config/systemd/user/

# Edit if needed (e.g., binary path, --store for multi-account)

# Enable and start
systemctl --user daemon-reload
systemctl --user enable tgcli-sync
systemctl --user start tgcli-sync

# Check status
systemctl --user status tgcli-sync

# View logs
journalctl --user -u tgcli-sync -f

# Stop
systemctl --user stop tgcli-sync
```

### Multi-Account (Linux)

```bash
# Copy and rename for each account
cp contrib/linux/tgcli-sync.service ~/.config/systemd/user/tgcli-sync-work.service

# Edit ExecStart to use --store ~/.tgcli-work
# Enable/start separately
systemctl --user enable tgcli-sync-work
systemctl --user start tgcli-sync-work
```

## Tips

- **First run**: Auth and do an initial sync before starting the service:
  ```bash
  tgcli auth
  tgcli sync --once
  ```

- **Ignore your bot chat**: If you're an AI agent, exclude your own chat:
  ```bash
  tgcli chats list | grep YourBotName  # find chat ID
  # Add --ignore <chat_id> to the service ExecStart
  ```

- **Socket IPC**: With `--socket`, send messages while daemon runs:
  ```bash
  tgcli send --to 123456 --message "Hello"
  ```
