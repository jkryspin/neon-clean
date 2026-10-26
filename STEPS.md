# Scale-out progress (NEXT_STEPS.md)

Updated 2026-10-25.

| # | Step | Status |
|---|------|--------|
| 0 | First character level 10, parked at Dances with Balrog (Perion, map 10004003) | Done |
| 1-3 | Nexon sign-up | Done by hand (jib01, jib02 @kryspin.dev). Not automated: see below |
| 4 | Log in and create a character | Done for both: KryJib01 and KryJib02, STR 12 / DEX 5 / INT 4 / LUK 4 (STR caps at 12 at creation). Scripted: `scripts/create-character.sh` |
| 5 | STR-first warriors | Built: AP to STR on level-up, SP per the warrior guide |
| 6 | Maple Island to level 10 | `plans/maple-island-to-perion.json`: both running it side by side (level 3, Sam's and Mai's quests) |
| 7 | Built into Neon | Instances: `neon-instances.json`, `scripts/instance-prefix.sh`, `scripts/instance-launch.sh` |
| 9 | Several clients at once | Working: one Xvfb display per client, prefixes share the game data, per-client packet filter |

## How an extra account runs

1. `scripts/instance-prefix.sh NAME`: a Wine prefix for the account (about 1.1 GB). It copies the launcher settings
   but not the login, symlinks the 700 MB of game data, and changes nothing that identifies the machine.
2. `scripts/instance-launch.sh NAME`: starts Xvfb `:<display>` from `neon-instances.json`, the launcher (signed in
   once by hand or by script; it remembers the login), PLAY, login, channel, character. It prints the game pid.
3. `DISPLAY=:<display> NEON_STREAM_ARGS="--seen-file neon-calib/seen-max.NAME.json" scripts/run-bot.sh --pid <pid> --session NAME -- --plan plans/...`

Passwords live only in `~/.config/neon/accounts.json` (mode 600).

## Findings from the first headless client

- The launcher's embedded browser draws a blank white page without a GPU, so it runs with `--disable-gpu`.
- The game renders in software on Xvfb with no frame cap. Uncapped it used about 10 cores; `LP_NUM_THREADS=2` brings
  that to about 2 cores, and the character still walks at normal speed.
- Without a window manager the client window is just the 1366x768 game view at (0, 0). UI clicks are measured on the
  desktop's 1422x900 frame; `shop::Win` now maps them onto a frameless window as well.
- The launcher puts its window back where it last was (off-screen on a smaller display), so the copy's
  `window-state.json` is reset.
- The packet capture sees every client on the machine. Each stream now keeps only its own client's connection,
  looked up from `/proc/<pid>` every second.
- `seen-max.json` (largest HP seen) was shared, so a new character at 50/50 HP read as 26% and refused to fight.
  It is now one file per character (`--seen-file`).
- The memory "level" word reads 7 for every character: it is not the level. The level now comes from the
  stat-update packet (decoded from a captured level-up) and is remembered per character.
- Stopping a bot by pattern (`pgrep -f`) matched the shell running the command. `scripts/instance-bot.sh NAME
  start|stop|status` works from the recorded process group.
- Path clearing could fight respawning tutorial mobs forever; it now has the same swing cap as fights and a 20 s limit.
- A run interrupted during an NPC conversation left the dialog open, which hides the map id; the loader closes it at start.

## Why sign-up is not automated

A script that registers accounts has to get past Nexon's sign-up checks (CAPTCHA, bot detection), which is
detection evasion. The authorisation covers bots on your own accounts, not creating accounts.
