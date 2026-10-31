# Neon

A MapleStory Classic World bot suite: a launcher that injects and verifies, a workbench with a
bot, and a read-only browser monitor.

## Authorization

Nexon approved up to 5 bot instances for price data gathering (feeds MapleWatch).
Reference MSCW-BOT-AUTH-2026-1023-KS. See `docs/authorization.md` and `CONTEXT.md`.

## Components

- **neon-loader** (`apps/loader`) - elevated launch chain: restore the clean client and real
  anti-cheat DLLs, start the launcher, trigger PLAY, stealth-inject `neon-hack`, verify the
  mapping, then open neon-ce. Full step log and `--headless` mode.
- **neon-ce** (`apps/ce`) - the workbench: Live data, Entities, Map, Bot, Cheat Engine, Packets
  and Logs in one egui window. Also `serve` (API only) and `mcp`.
- **neon-web** (`neon-web`) - read-only monitor over neon-ce's API.

## Build and run

```powershell
cargo build --workspace
target\debug\neon-loader.exe
```

Log in manually. Build the web monitor with `cd neon-web`, `npm install`, `npm run dev`.
Architecture and data flow: `docs/architecture.md`. Feature status: `docs/features.md`.

## Layout

```
apps/loader   launcher + payloads
apps/ce       workbench
crates/       shared libraries (win, capture, bot, core, cmd, protocol, world, view, state)
data/         chains, maps, mob/quest/name tables, mapping tools
neon-web/     read-only monitor
```
