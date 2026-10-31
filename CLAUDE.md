# Neon - MapleStory Bot Suite

Two components only: **neon-loader** (launch, patch, inject, verify) and **neon-ce** (workbench
and bot). **neon-web** is a read-only browser monitor. Architecture: `docs/architecture.md`.

## Quick Start

```powershell
cargo build --workspace
target\debug\neon-loader.exe        # elevated; opens neon-ce after a verified injection
target\debug\neon-ce.exe --pid N    # or attach from inside the window
```

Log in manually. `neon-loader --headless` prints the step log instead of showing a window.
`neon-ce serve` runs the API without a UI; `neon-ce mcp` is the MCP bridge.

## Where things are

- `apps/loader` - launch chain, C payloads in `payload/`, injection status UI
- `apps/ce` - Live, Entities, Map, Bot, Cheat, Packets, Logs tabs
- `crates/neon-bot` - brain, plans, navigation, safety, Windows input
- `crates/neon-capture` - chains, footholds, contact scan, packet codec
- `crates/neon-win` - process, SendInput, window capture
- `data/` - chains, maps, mob/quest/name tables, tools (formerly `neon-calib`)
- `neon-web` - `npm install && npm run dev`, monitor only, localhost by default

## Authorization

Nexon approved up to 5 bot instances on MapleStory Classic World for price data gathering
(feeds MapleWatch). Reference MSCW-BOT-AUTH-2026-1023-KS. See `docs/authorization.md`.

## Current status

- B1 launch path is proven live: clean stock client, real anti-cheat DLLs
  (`grap64`/`nmcogame64`), `BlackCat1` running, stealth payload mapped. b2 (mocks + NGS-skip)
  remains an alternative.
- Live reads: HP/MP/level/position/map/grounded/footholds. Entities are unclassified contact
  rectangles; packet capture is optional.
- Bot executes in-process from CE with the safety gate, anti-detect, plans, navigation and
  shop clicks. Manual login; Windows input through SendInput.
- No packets are sent. Lie detector, GM evasion and outgoing packets remain incomplete.

## Rules of thumb

- Run elevated; the process DACL blocks memory access otherwise.
- Re-verify `data/chains.json` after any client patch: `neon-ce` Live tab plus
  `docs/data-sources.md`. Unreadable roots mean the client changed.
- gdb attaches cost sessions; prefer passive reads.
- The web API is read-only. Start/stop spawns or kills neon-ce.
