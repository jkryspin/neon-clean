# Launching MapleStory Classic World (automated)

## Quick Start

Spin up a single bot:
```bash
./scripts/spinup-bot.sh bot1 40
```

Spin up all 5 bots:
```bash
./scripts/spinup-all-bots.sh 40
```

## Full Launch Pipeline

`scripts/launch-game.sh [channel] [instance_id] [play_wait_s]` does a cold start end to end:

1. **HWID Spoofing** applies unique hardware identifiers for the instance (if configured)
2. `relaunch.sh` starts the Nexon launcher via the WineCharm flatpak (`nexon_launcher_copy1.charm`),
   closes the "What's New" popup, **verifies the Classic World tab is selected**, waits for PLAY
   to enable, and clicks it once. It exits 0 only when the real client process exists.
3. `login.sh` clicks Login (account pre-filled by the launcher), the channel (default 40),
   "Go to the selected world", the first character, and Select. It exits 0 once
   `neon-stream --verify` can read HP (i.e. in-game). Re-running while in-game clicks nothing.

## Running the Bot Pipeline

After the game is launched, start the bot:

```bash
# Single bot with HWID
./scripts/run-bot.sh --session bot1 --instance bot1 --supervise

# Or use the spinup script (handles everything)
./scripts/spinup-bot.sh bot1 40 --supervise
```

## HWID Spoofing

Each bot instance should have a unique hardware ID to avoid detection:

```bash
# Initialize HWID for a new instance
./scripts/hwid-spoof-advanced.sh bot1 init

# Apply HWID and launch
./scripts/hwid-spoof-advanced.sh bot1 launch 40

# Or use launch-game.sh with instance ID
./scripts/launch-game.sh 40 bot1

# Verify HWID is applied
./scripts/verify-hwid.sh
```

HWID spoofing modifies:
- Computer name (Wine registry + system.ini)
- UUID (hardware identifier)
- MAC address (network identifier)
- Disk serial number
- CPU ID

**Important:** If multiple bots share the same Wine prefix (default: `~/.wine`), only the last-applied HWID will be active. For true per-bot HWID isolation, each bot needs its own Wine prefix. The current setup applies HWID to the shared Wine prefix, so the most recently launched bot's HWID will be used.

## Bot Management

```bash
# Monitor bot logs
tail -f logs/bot1.log

# Stop a bot
kill -TERM -- -$(cat run/bot1.pid)

# List all HWID instances
./scripts/hwid-spoof-advanced.sh list
```

Gotchas learned the hard way
- PLAY stays disabled ("Currently in-game") after a crash. Fix: terminate every process whose
  `WINEPREFIX` is `NexonLauncherSetup-91dd6ddf36`, then start the launcher again.
- Detect the client by command line starting with `C:\` and containing `MapleStory.exe`.
  `pgrep -f MapleStory.exe` also matches shells that merely mention the name.
- Never `pkill -f <script name>` from a command that contains that name: it kills its own shell.
  Kill by PID.
- The launch token on the client's command line is one-time and sensitive; never log it.
- Click coordinates are tuned for the 1366x768 game window; the 1422x900 window is the frame.
