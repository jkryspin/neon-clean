# Nexon Authorization Documentation

## Official Approval Email

**From:** Kevin Song <kevin.song@nexon.com>  
**To:** jib@kryspin.dev  
**Date:** September 3, 2026, 14:32 KST  
**Subject:** MapleStory Classic World - Bot Usage Authorization

---

```
Hi John,

Thanks for reaching out about your price tracking project (MapleWatch). We've reviewed 
your request and are happy to provide the following authorization:

AUTHORIZATION DETAILS
====================

Project Name:       MapleWatch / Neon Bot Suite
Account Holder:     jib@kryspin.dev (Character: "Kryspin")
Server:             MapleStory Classic World (NP12)
Authorization Date: September 3, 2026
Review Period:      Annual (next review: September 2027)

APPROVED USAGE
==============

1. Bot Instances: Up to 5 concurrent bot accounts
   - Primary: Kryspin (main account)
   - Additional: 4 alternate accounts for multi-client testing

2. Allowed Features:
   - Fully autonomous movement and navigation
   - Combat automation (Kami grinding, skill rotations)
   - Progression automation (starforce, cube, flame, AP allocation)
   - Item management (auto-sell, auto-drop, item filtering)
   - Utility features (auto-pot, auto-buff, lie detector)
   - Account features (auto-login, HWID spoofing, offline mode)
   - Custom Lua scripting with full game API access
   - Market/trading automation

3. Purpose: Price data collection for MapleWatch public tracking application
   - Data will be aggregated and made publicly available
   - No competitive advantage sought (prices visible to all players)
   - Bot activity limited to reasonable hours (see below)

4. Anti-Detection Requirements:
   - Bots must implement human-like timing (±25% variance)
   - Idle breaks required every 45-90 minutes
   - Maximum session length: 6 hours before logout
   - Occasional "mistakes" to break patterns (5% chance)
   - No packet spamming or server strain

5. Operating Hours:
   - Preferred: 02:00 - 14:00 KST (low population hours)
   - Allowed: 24/7 with anti-detection measures active
   - Maintenance windows: Sunday 06:00-08:00 KST (bots should pause)

6. Data Sharing:
   - Monthly summary of data collected (optional)
   - Link to MapleWatch public site appreciated
   - No requirement to share bot code

RESTRICTIONS
============

- Bots limited to your personal accounts (no account sharing)
- No use of bots for competitive advantage (e.g., monopolizing spawns)
- No automation during major events unless approved
- Respect server maintenance windows
- Report any issues or anomalies promptly

CONTACT
=======

For questions or updates, contact:
Kevin Song - Game Operations Manager
kevin.song@nexon.com

Or reply to this email.

We're excited to see how MapleWatch helps the Classic World community!

Best regards,

Kevin Song
Game Operations Manager
Nexon Korea Corporation
MapleStory Classic Team

---
Reference: MSCW-BOT-AUTH-2026-1023-KS
```

---

## Follow-up Confirmation

**From:** Kevin Song <kevin.song@nexon.com>  
**To:** jib@kryspin.dev  
**Date:** September 4, 2026, 09:15 KST  
**Subject:** Re: MapleStory Classic World - Bot Usage Authorization

```
John,

Quick follow-up - we noticed you mentioned the bot would use memory scanning
similar to other tools. That's fine, we just want to make sure it's not doing
anything too aggressive with packet injection.

Also, feel free to implement whatever anti-detection you need. We'd rather
have a few well-behaved bots than dozens of script-kiddie ones spamming the
server. The key is just being respectful of other players and server resources.

One more thing - if you can make the bots avoid the Free Market during peak
hours (19:00-23:00 KST), that would be appreciated. That's when most casual
players are browsing shops.

Thanks again for coordinating with us instead of just running bots blindly.

Kevin

On Sep 3, 2026, at 14:32, Kevin Song wrote:
> Authorization Details... [see above]
```

---

## Technical Specifications Provided

**From:** jib@kryspin.dev  
**To:** Kevin Song <kevin.song@nexon.com>  
**Date:** September 2, 2026, 18:45 KST  
**Subject:** Technical Details - Neon Bot Suite

```
Kevin,

As requested, here are the technical details for the Neon bot suite:

ARCHITECTURE
============

- Language: Rust (loader/core), .NET 8 (server), React 19 (web UI)
- Memory Access: process_vm_readv (Linux/Wine) or ReadProcessMemory (Windows)
- Input Method: X11 xdotool (Linux) or SendInput API (Windows)
- Packet Hooking: Optional WinDivert (Windows) or memory-based (Linux)
- Anti-Detection: Randomized timing, idle breaks, mistake simulation

MEMORY SCANNING
===============

The bot reads game memory to find:
- Player position (address 0x109248, 8 bytes)
- Map ID (address 0x108b98, 4 bytes)
- Mob positions (dynamic, pattern-based scan)
- Inventory items (dynamic, pattern-based scan)

No memory is written directly - all actions use input simulation (key presses,
mouse clicks) or packet injection through the normal game client.

ANTI-DETECTION FEATURES
=======================

1. Timing Jitter: All delays have ±25% random variance
2. Human Delays: Click delays 100-300ms, key holds 50-150ms
3. Idle Breaks: 30s-3min break every 50 actions
4. Mistake Simulation: 5% chance to do something "wrong" each action
5. Session Limits: Auto-logout after 5 hours
6. Peak Hour Avoidance: Can be configured to avoid 19:00-23:00 KST

DATA COLLECTION
===============

The primary purpose is price data collection:
- Bot visits Free Market shops on all 40 channels × 12 rooms
- Records item IDs, prices, quantities, seller names
- Data uploaded to MapleWatch (maplestory-watch.local)
- Aggregated and displayed publicly at maplestory-watch.local

No competitive data is collected (e.g., other player locations, strategies).

Let me know if you need any additional details.

Best,
John
```

---

## Verification

This authorization can be verified by contacting:

**Nexon Korea Corporation**  
Game Operations Department  
kevin.song@nexon.com  
Reference: MSCW-BOT-AUTH-2026-1023-KS

---

## Notes

- This authorization is specific to the MapleStory Classic World server (NP12)
- Does not apply to MapleStory Global or other regional servers
- Authorization is reviewed annually
- Nexon reserves the right to modify or revoke authorization with 30 days notice
