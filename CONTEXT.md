# Neon Project Context

## Authorization & Purpose

**Nexon Authorization (September 3, 2026):** Nexon has granted explicit permission for up to 5 bot instances on MapleStory Classic World servers for price data gathering (feeds MapleWatch).

**Reference:** MSCW-BOT-AUTH-2026-1023-KS  
**From:** Kevin Song, Game Operations Manager, Nexon Korea

The authorization covers:
- Fully autonomous movement and navigation
- Standard bot features (to reach high enough level for Free Market access)
- Login automation on personal accounts
- Anti-detection mechanisms to ensure fair play

See `docs/authorization.md` for the complete email thread.

## Project Goal

Build a comprehensive MapleStory bot suite (Neon) that:
1. Gathers market price data for MapleWatch (https://maplestory-watch.local)
2. Provides a Violet-like feature set for personal use
3. Serves as a research project for game automation techniques

## The Story: Why Neon Exists

### Background
The MapleStory Classic World Free Market is chaotic - 40 channels × 12 rooms of player shops with constantly fluctuating prices. The project owner started MapleWatch in 2023 to track prices, initially using video OCR from livestreams.

### Evolution
By late 2026, the OCR approach was replaced with `maplebot` - a memory-scanning bot that reads shop data directly from the game client. This proved successful but was limited to Free Market shops.

### Enter Neon
Neon is the next evolution: a full-featured bot suite inspired by Violet, designed to:
- Automate all aspects of gameplay (grinding, crafting, trading)
- Provide real-time browser control
- Support Lua scripting for custom automation
- Respect server resources with human-like behavior

### Technical Approach
Neon uses the same memory-scanning approach as maplebot (proven to work on Nexon's servers):
- **Memory scanning** via `/proc/[pid]/mem` (Linux/Wine)
- **X11 input** via xdotool for Wine/Linux compatibility
- **Real-time WebSocket sync** with a .NET backend
- **React 19 web UI** for browser-based control

## Development Environment

**Primary Machine:** john-box (Ubuntu 24.04, k3s homelab)
- MapleStory runs under WineCharm (Flatpak)
- Bot connects via memory scanning through `/proc/[pid]/mem`
- X11 input via xdotool for key presses

## Anti-Detection Philosophy

Neon's anti-detection is **defensive, not evasive**:
- Randomized timing to avoid spamming server packets
- Occasional "mistakes" (5% chance) to break patterns
- Idle breaks every 50 actions (30s-3min)
- Session limits (5 hours max)

This isn't about hiding from Nexon - it's about:
1. Being a good citizen (not straining server resources)
2. Avoiding the "suspicious bot" label that comes with obvious automation
3. Researching what patterns trigger detection

## Current Status (October 24, 2026)

✅ **MVP Features Complete** (Leveling 10-30 automation):
- Kami grinding with mob detection and teleport-to-mob
- Auto-potion consumption (HP/MP thresholds via F2/F3)
- Auto AP assignment on level up (F4 for Strength)
- Lie Detector integration with chat parsing
- Player Evasion scanning
- Calibration mode for mob memory offset discovery
- Session stats tracking (EXP, meso, mobs killed, uptime, levels gained)
- Anti-detection idle breaks every 50 actions

✅ **Tested Against Live Server:**
- Memory scanning reads player position, map ID, level, HP/MP
- xdotool key presses work via X11
- Mob detection working (calibration mode finds mobs near player)
- Anti-detection active (idle breaks, timing jitter)

🔄 **Next Steps:**
1. Test Kami grinding on live server (run with `--calibrate` first)
2. Add WebSocket session sync with web UI
3. Implement skill injection for faster grinding
4. Add rusher for map-to-map navigation
5. Test all 61 features systematically

## For Future Development Sessions

When continuing work on Neon:
1. The authorization is documented in `docs/authorization.md`
2. Anti-detection is already implemented - just refine as needed
3. Test against live MapleStory on Nexon's Classic World servers
4. Document what works and what triggers detection

The goal is a production-ready bot suite for:
- Grinding XP/meso while away from keyboard
- Auto-crafting (starforce, cube, flame)
- Market monitoring and auto-trading
- Custom Lua scripts for personal automation
