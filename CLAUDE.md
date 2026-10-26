# Neon - MapleStory Bot Suite

## Quick Start

1. Read `CONTEXT.md` for project background
2. Check `PLAN.md` for current status and next steps
3. MapleStory runs on john-box under Wine
4. Read `docs/data-sources.md` for what the bot can actually read from the game, what is
   verified, and what is still missing. The "61 features" in PLAN.md are code that exists,
   not features proven against the live client; only the data pipeline below is.
5. Restarting the client: `docs/launching.md` (`scripts/launch-game.sh`)

## Authorization

Nexon approved up to 5 bot instances on MapleStory Classic World for price data gathering (feeds MapleWatch).

**Reference:** MSCW-BOT-AUTH-2026-1023-KS  
**Date:** September 3, 2026  
**From:** Kevin Song, Game Operations Manager, Nexon Korea

See `docs/authorization.md` for the full email thread.

## Current Status (2026-10-24)

Rewritten after live testing; the earlier "MVP complete" list overstated what was proven.
Details and evidence: `docs/data-sources.md`.

**Working and tested (unit tests + live read-only runs):**
- `neon-stream`: reads position, HP/MP, level, grounded, map ID through static roots that
  survived a client restart and map changes; map geometry (footholds) and entity hitboxes by
  bounded signature scan. 41 unit tests (incl. real captured packets).
- `neon-loader` brain (`brain.rs`): pure state-in/actions-out decision logic with tests for
  grinding, potions with cooldowns, AP on level-up, map change, stranger pause, low-HP stop,
  and looting (walk to a same-floor drop, press the pickup key, give up on stuck drops). 28 unit tests. Dry-run against the live stream works; it has **not** been run sending keys.
- Client relaunch/login automation: `scripts/launch-game.sh`, `docs/launching.md`.

**Not working / not proven:**
- Mobs (with template ids) and other players (with names) now come from the network (`--net`), verified live; they have no memory root. EXP and own drops are decoded too (stat update `0x007e`, drops `0x0471`); AP, max HP/MP, player removal, and packets sent *to* the server are not verified.
- No EXP, AP, drops, other-player names, or rope flag; max HP/MP only remembered per level.
- Navigation (platform graph from the client's map data, ladders/ropes, drop-through), 64 px punch range, XTEST key input, 50 fps stream: live-tested on Forest West of Henesys (see docs/navigation.md).
- Lie detector, player evasion data, buffs, skills, teleport/"kami", rusher: code exists in
  `neon-core` but is not wired to real data or tested live.
- A level-7 test character died during map-change testing. The brain now stops fighting below 30% HP; keep that guard on for any live run.

## Control panel and features

`docs/features.md` lists every feature, the Violet comparison, the control files/API and the plan format.
`cd neon-web && npm run dev` serves the panel on http://127.0.0.1:3000 (Start/Pause/Stop, live settings, map rusher,
scheduler, history, profiles). Start the bot from there or with `scripts/run-bot.sh --supervise`.

## Running (current pipeline)

```bash
# Spin up a single bot with HWID spoofing
./scripts/spinup-bot.sh bot1 40 --supervise

# Spin up all 5 bots
./scripts/spinup-all-bots.sh 40 --supervise

# Or manually:
# 1. Apply HWID
./scripts/hwid-spoof-advanced.sh bot1 apply

# 2. Launch game
./scripts/launch-game.sh 40 bot1

# 3. Run bot pipeline
./scripts/run-bot.sh --session bot1 --instance bot1 --supervise
```

Legacy commands (older architecture, kept for reference):

```bash
# Start server
cd neon-server && dotnet run --urls "http://0.0.0.0:5001"

# Start web UI  
cd neon-web && npm run dev

# Run bot (needs DISPLAY for xdotool)
export DISPLAY=:0
cd neon-loader && cargo run --release -- --session <SESSION_ID>
```

## Key Files

- `neon-loader/src/` - Memory scanner, input controller, bot logic
- `neon-core/src/` - Feature modules (combat, movement, inventory, etc.)
- `neon-server/` - .NET 8 API with SQLite
- `neon-web/src/` - React 19 web UI

## Anti-Detection Features

The bot includes anti-detection features for extended unattended runs:
- Randomized timing to avoid spamming server packets
- Idle breaks every 50 actions (30s-3min)
- Session limits (5 hours max)
- Occasional "mistakes" (5% chance) to appear human-like
- GM reaction: pause when staff is detected on the map (Phase 8)
- Lie detector solver: automatically answer anti-macro checks (Phase 6)
- Outgoing packet support: send packets for features like chat and emotes (Phase 9)

These features are authorized under MSCW-BOT-AUTH-2026-1023-KS.
