# tgcli Reference

Extended command reference for less-common operations. See [SKILL.md](SKILL.md) for core workflows.

## Stickers

List, search, and send stickers.

```bash
tgcli stickers list --output markdown   # List sticker packs (markdown)
tgcli stickers search "cat"             # Search sticker sets
tgcli stickers send --to 123456789 --sticker CAT_ABC123  # Send sticker
```

## Folders

Create and manage chat folders.

```bash
tgcli folders list --output markdown   # List folders (markdown)
tgcli folders create "Work Chats"      # Create new folder
tgcli folders delete 5                 # Delete folder by ID
```

## Admin (Groups/Channels)

Ban, kick, promote, demote members. These affect real users — verify the target user ID first.

```bash
tgcli admin ban --chat 111222333 --user 999888777       # Ban user
tgcli admin kick --chat 111222333 --user 999888777      # Kick user
tgcli admin unban --chat 111222333 --user 999888777     # Unban user
tgcli admin promote --chat 111222333 --user 999888777   # Promote to admin
tgcli admin demote --chat 111222333 --user 999888777    # Demote admin
```

## Daemon (Real-Time)

Listen for real-time updates from Telegram servers. Optional — use `sync` for most workflows.

```bash
tgcli daemon                    # Listen for updates
tgcli daemon --stream           # JSONL output
tgcli daemon --no-backfill      # Skip background sync
tgcli daemon --ignore 987654321 # Ignore specific chat
tgcli daemon --ignore-channels  # Skip all channels
```

## Utility Commands

```bash
tgcli read --chat 987654321              # Mark chat as read
tgcli typing --chat 987654321            # Send typing indicator
tgcli profile show                       # Show your profile
tgcli profile set --first-name "Alex"    # Update your name
tgcli completions bash                   # Shell completions
```
