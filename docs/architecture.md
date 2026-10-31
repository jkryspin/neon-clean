# Architecture

Two application components, one workspace, one UI system.

```
apps/loader      neon-loader.exe   launcher: patch, mock, inject, verify, open CE
apps/ce          neon-ce.exe       workbench: live, entities, map, bot, cheat, packets, logs
crates/neon-win                    process/memory, windows, SendInput, capture, hashes
crates/neon-capture                chains, geometry, contact rectangles, packet codec
crates/neon-bot                    brain, plans, navigation, safety, Windows input
crates/neon-core | neon-cmd        anti-detect and safety helpers used by neon-bot
crates/neon-protocol | neon-world  opcode registry and packet model for the Packets tab
crates/neon-view                   optional packet viewer library
data/                              chains.json, maps, mob data, name tables, quests
neon-web/                          read-only browser monitor over neon-ce's API
```

## Runtime

1. Start `neon-loader` elevated.
2. It restores the clean client and the real anti-cheat DLLs, starts the Nexon launcher,
   triggers PLAY, stealth-injects `neon-hack`, and verifies the mapping (private region,
   PEB-unlinked, wiped headers, read-only header, RX text).
3. It opens `neon-ce --pid <pid>` once every check passes (unless auto-open is off).
4. neon-ce owns the process handle. Each tab reads snapshots from one worker; the egui
   thread only renders them.

## Entity source

Mobs, players and drops are read from process memory. There is no static entity root yet:
the Contact tab uses the collision-rectangle signature scan (`neon-capture/src/rects.rs`) and
filters feet against the map's footholds. Classified pointer chains belong in
`data/entities.json`, using the same status vocabulary as `chains.json`. Packet capture is
optional and currently only backs the opcode browser.

## API

`neon-ce` serves a read-only HTTP API on `127.0.0.1:8790` (`--port`):

* `GET /api/live`, `/api/state`, `/api/events` (SSE), `/api/logs`, `/api/health`

The JSON-RPC MCP engine API stays on `127.0.0.1:8791` with a bearer token in the discovery
file. `neon-web` only talks to the read-only surface.

## Invariants

* The launcher never reads game state after injection; CE owns the handle.
* One loader+CE pair per client; the port advances by instance.
* The bot runs in a supervised worker thread, never on the UI thread.
* Nothing sends packets yet; the bot acts through `SendInput` and window clicks.
