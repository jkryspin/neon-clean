# Neon

A full-featured MapleStory botting application with 61 features, browser-based control, and Lua scripting.

## Authorization

**Nexon Approved (2026-10-23):** This project has explicit permission from Nexon to run up to 5 bot instances on MapleStory Classic World servers for price data gathering (feeds MapleWatch). The authorization covers:
- Fully autonomous movement and gameplay automation
- All standard bot features (Kami grinding, auto-crafting, etc.)
- Anti-detection mechanisms for fair play
- Login automation on personal accounts

**Reference:** MSCW-BOT-AUTH-2026-1023-KS

See [docs/authorization.md](docs/authorization.md) for the complete authorization email thread and [CONTEXT.md](CONTEXT.md) for the full project background.

## Features

### Combat & Botting (10)
- **Kami** — teleport-to-mob grinding with targeting modes and pickup
- **Skill Injection** — packet-level rotations with per-skill delays and safe mode
- **Rotation Builder** — generate a loop from the map's spawns in one press
- **God Mode** — stop taking damage, with a variant tuned for bosses
- **Full Map Attack** — hit everything on the map at once
- **Missile Storm** — one skill, fired as many missiles
- **Auto Attack** — presses your attack key on a timer
- **No Delay** — attack delay removed, and it repositions at the cap
- **Movement Unlocks** — infinite flash jump, no air check
- **Mob Disarm** — mobs stop attacking back

### Progression Automation (5)
- **Auto Starforce** — stars to target with safeguard, estimates, and meso protection
- **Auto Cube** — rolls until your AND/OR conditions are met
- **Auto Flame** — rerolls flames, keeps only upgrades
- **Auto Inner Ability** — circulates to grade and line goals, locks matches
- **Auto AP** — ability points assign themselves

### Items & Loot (4)
- **Item Filter** — whitelist or blacklist drops by ID, category, or meso value
- **Auto Sell** — clears listed items at shops, or rushes to one and back
- **Auto Drop** — loots everything, drops the junk, keeps quest counters
- **Pet Loot** — pet teleports to drops for instant pickup

### Utility & Navigation (9)
- **Auto Lie Detector** — solves every anti-macro check, pausing risky features meanwhile
- **Scheduler** — steps, reset-day tasks and safety rules for a whole day of play
- **Auto Rune** — runes solve themselves
- **Rusher** — pathfinds map to map, routes by level, finds any map by name
- **Auto Pot** — potions on HP and MP thresholds
- **Auto Buff** — buffs recast before they drop
- **Auto NPC** — clicks through NPC dialogs and notification boxes
- **Spawn Points** — land on your chosen coordinates per map
- **Auto Pet Care** — feeds and resummons pets

### Accounts & Multi-Client (9)
- **Multi-Client Sessions** — one session included, add unlimited slots on any plan
- **Game Profiles** — RSA-2048 credentials encrypted in your browser, MFA at launch
- **Auto Login** — world, channel, character slot, done
- **Auto Relaunch** — client dies, session comes back
- **Auto Char** — creates characters with class, name, and intro skip
- **Account Bank** — your own spare logins, imported in bulk and swapped onto profiles
- **Proxy Profiles** — SOCKS5 or HTTP proxy per game account
- **HWID Spoofing** — per-account hardware ID that follows the account
- **Offline Mode** — log in without appearing online

### Safety & Evasion (4)
- **Player Evasion** — react to strangers: change channel, disable, logout, terminate
- **IGN Whitelist** — trusted players, your own alts auto-added
- **Panic Mode** — one key kills everything, one key restores it
- **Dangerous-Feature Gating** — ban-risk toggles stay hidden until you opt in

### Lua Scripting (8)
- **Lua Plugin Scripts** — tick, packet, map, chat, and dialog callbacks
- **Full Game API** — player, mobs, map, skills, inventory, quests, trades, endgame
- **Custom Menu Panels** — scripts build their own tabs and controls
- **In-Browser Editor** — edit live, syntax-checked, hot reload
- **require() Modules** — multi-file libraries and marketplace imports
- **HTTP & Webhooks** — async requests and Discord pings from scripts
- **Packet Tools** — intercept, craft, block, and watch packets live
- **Lua Documentation** — full SDK docs plus an LLM-ready reference

### Profiles & Live Control (7)
- **Real-Time Browser Control** — every change lands in-game instantly
- **Settings Profiles** — named configs, auto-load per session, shareable as codes
- **Multi-Computer Control** — every rig, one panel, remote launch
- **Macros & Keybinds** — bind any setting to any key, M4/M5 included
- **Session Stats** — live EXP and meso rates, per client
- **Console & Activity Log** — live client log and per-computer event history
- **Performance Tweaks** — hide effects and rendering to cut CPU

### Marketplace & Community (5)
- **Script Marketplace** — free and paid community scripts, installed in a click
- **Shared Settings Profiles** — community profiles, one-click install
- **Free Trials** — try paid products once before buying
- **Safety Badges** — dangerous or network access, flagged up front
- **Publish Your Own** — list your scripts with pricing, trials, and earnings

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              NEON LOADER (Rust)                             │
│  DLL Injector | Packet Hook (WinDivert) | Memory Scanner                   │
└─────────────────────────────────────────────────────────────────────────────┘
                               │
┌─────────────────────────────────────────────────────────────────────────────┐
│                      NEON CORE (Rust)                                       │
│  Combat | Movement | Inventory | UI Click | Scheduler                       │
└─────────────────────────────────────────────────────────────────────────────┘
                               │ WebSocket
┌─────────────────────────────────────────────────────────────────────────────┐
│                          NEON SERVER (.NET 8)                               │
│  Session Manager | Script Host (Lua) | Profile Store | Marketplace API     │
└─────────────────────────────────────────────────────────────────────────────┘
                               │ HTTP/WebSocket
┌─────────────────────────────────────────────────────────────────────────────┐
│                          NEON WEB UI (React 19)                             │
│  Dashboard | Script Editor | Settings | Marketplace | Multi-Client Manager │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Getting Started

### Prerequisites
- Rust 1.75+ (`rustup install stable`)
- .NET 8 SDK (`dotnet --version`)
- Node 20+ (`node --version`)
- MapleStory Classic World client (Wine or native Windows)

### Build

```bash
# Build loader and core
cd neon-loader && cargo build --release
cd ../neon-core && cargo build --release

# Build server
cd ../neon-server && dotnet build

# Build web UI
cd ../neon-web && npm install && npm run build
```

### Run

```bash
# Start server
cd neon-server && dotnet run

# Open browser
# http://localhost:5000
```

## Plan

See [PLAN.md](PLAN.md) for the full implementation plan.

## License

MIT
