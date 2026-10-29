# Neon: DLL Injection, Game Manipulation & Violet Feature Parity

## Context

The bot suite needs packet-level game manipulation (god mode, skill injection, kami, etc.)
to match Violet's feature set and grind a warrior to level 30. The prerequisite chain:

**fix injector → mock anti-cheat → capture session tokens → find iframe timer → map wire opcodes → send packets → implement all features → level 30**

Under Wine/CrossOver, the real anti-cheat kernel components (Themida ring-0) don't load,
so the game already runs. The single biggest blocker after injection is **per-connection
wire opcode scrambling** — without mapping wire↔internal opcodes, no packets can be sent.

## Violet Feature Parity

From `docs/features.md`, the Neon system currently covers these Violet features:

| Violet Feature | Neon Status | Dependency |
|---|---|---|
| Auto Farm (fights, monitors HP/MP/potions) | ✅ Yes (pixel-based) | Already works |
| Map Rusher (travel to any map) | ✅ Yes (portal graph) | Already works |
| Route safety check | ✅ Yes | Already works |
| Task Scheduler | ✅ Yes | Already works |
| Party Sync | ⚠️ Partly | Already works |
| Keep stocked / shop trips | ✅ Yes (UI clicks) | Already works |
| Item filter | ⚠️ Partly | Already works |
| Rotation Builder | ⚠️ Partly | Already works |
| Multi-client sessions | ✅ Yes | Already works |
| Safety: strangers, friend whitelist | ✅ Yes | Already works |
| Safety: panic key | ✅ Yes | Already works |
| Safety: connection loss | ✅ Yes | Already works |
| Safety: stop rules, Discord DMs | ✅ Yes | Already works |
| Macros and keybinds | ✅ Yes | Already works |
| Live browser control | ✅ Yes (neon-web) | Already works |
| Live game view | ✅ Yes (MJPEG) | Already works |
| Auto Login & Relaunch | ⚠️ Partly | Already works |
| Script marketplace | ⚠️ Partly | Already works |
| Lua scripting API | ✅ Rules engine | Already works |
| Out packets (`out_packet(...):send()`) | 🔴 NOT IMPLEMENTED | **This is the blocker** |

**Phase 6 (Packet Injection) targets the one major gap: sending outgoing packets.**
Once implemented, it unlocks:
- God mode (freeze iframe timer via memory write)
- Skill injection (AttackSkill packets)
- Kami teleport (MoveSelf packets)
- Full map attack (extended range)
- No-delay attack (bypass cooldown)
- Flash jump (flight flags)
- Mob disarm (incoming filter)

These are **not** in Violet's feature list — they are Neon's own advantages over pixel-based
automation, enabling server-level manipulation without UI interaction.

## Architecture

```
Phase 1: Fix Injector ──→ Phase 2: Mock Anti-Cheat
                     └──→ Phase 3: Session Token
                     └──→ Phase 4: God Mode (memory-only)
                     └──→ Phase 5: Opcode Probe ──→ Phase 6: Packet Features
                                                          │
Phase 7: Non-injection features (parallel) ──→ Phase 8: Level 30 ←──┘
                                               │
                                        Phase 6 features feed in
```

## Phase 1: Fix Undetectable DLL Injector

**File:** `neon-probe/inject-undetectable.c`
**Status:** ✅ DONE (2026-10-29)

Rewrote the manual PE mapper fixing all 6 bugs:
1. Added `apply_relocations()` — walk `.reloc` directory for DIR64/HIGHLOW entries
2. Rewrote `resolve_imports()` — reads from LOCAL image copy, not target process memory
3. Fixed `hijack_thread()` — targets game process threads, not injector's
4. Added shellcode stub with proper x64 calling convention (RCX/RDX/R8)
5. Fixed `MEM_RELEASE()` → `MEM_RELEASE`
6. Added `section_to_page_protect()` — converts PE characteristics to PAGE_* constants

Flow: local image prep (sections + relocations + imports) → `WriteProcessMemory` → shellcode → thread hijack → poll for completion → restore.

**Build:** `make cross` in `neon-probe/`

## Phase 2: Mock Anti-Cheat DLL

**Status:** ✅ IMPLEMENTED (2026-10-29) — see `docs/anticheat-mock.md`

Anti-cheat is GRAP, not NGS/BlackCipher. `MapleStory.exe` imports exactly two anti-cheat
symbols (`grap64.dll` ordinal 9, `nmcogame64.dll!NMCO_MemoryFree`); mocks for those two
remove the protector from the process. Under Wine/CrossOver the real kernel components
(`BlackCat64.sys`, `NGService.exe`) can't load and the real `grap64.dll`,
`grap-communicator64.aes`, `grap-core64.aes` load degraded — stable with probe DLLs
(9+ min, no kicks) — so the mocks are an explicit unprotect on top of that.

Files: `neon-probe/mock-grap64.c`(+`.def`), `neon-probe/mock-nmcogame64.c`(+`.def`),
installed/restored by `scripts/mock-anticheat.sh`.

Loaded anti-cheat modules (when real, 99 total modules in process):
- `grap64.dll` at `0x6FFFFBF60000` — 9 ordinal-only exports (Themida-packed)
- `grap-communicator64.aes` at `0x6FFFEDC00000` — DLL, 1 ordinal export, no names
- `grap-core64.aes` — separate process (Wine PID ~2124), 1 named export
- `nmcogame64.dll`, `nmconew64.dll` — Nexon protection modules (loaded, passive)

## Phase 3: Session Token Capture

**Status:** ✅ FRAMEWORK IMPLEMENTED (2026-10-29)

### Approach: CreateProcessW Hook DLL

**File:** `neon-probe/hook-session.c`

A DLL that hooks `CreateProcessW` in `nexon_client.exe` to capture the session token
passed when the launcher spawns `MapleStory.exe`.

**Token extraction patterns:**
1. Named parameters: `--token=`, `--session=`, `?token=`, `?session=`, `session_token=`, `access_token=`
2. Fallback: long hex string detection (32+ contiguous hex chars)

**Storage:** Named shared memory (`NeonSessionToken`) — never written to disk, per authorization terms.

**Exports:**
- `neon_get_session_token(char *out, size_t out_size)` — read the token
- `neon_clear_session_token(void)` — clear the token from memory

**Compile:** `make -C neon-probe hook-session-win.dll`

### Real detour (implemented)

The hook is a real IAT patch: it enumerates every module in the injected process and
rewrites import slots that point at `kernel32`/`kernelbase` `CreateProcessW`/`A`, then
trampolines to the original. No code bytes are patched. Inject with
`inject.exe --all nexon_client.exe` (also `nexon_runtime.exe`, `nexon_launcher.exe`) via
`scripts/mock-anticheat.sh inject`.

### Alternative: macOS `ps` Approach
```bash
ps -o args= -p $(pgrep -n MapleStory.exe)
```
Parse the command line for token patterns. Less reliable (token may be in environment, not args).

### Integration with neon-cmd
`neon-cmd` calls `neon_get_session_token()` via a shared library interface to obtain the token
for headless connection authentication.

## Phase 4: God Mode (iframe Timer)

**Status:** ✅ FRAMEWORK IMPLEMENTED (2026-10-29)

### Differential Memory Scanning

**File:** `neon-stream/src/scanner.rs`

Extended with `GodModeScanner` — a differential scanner that finds the iframe timer
by observing memory changes around damage events.

**Algorithm:**
1. **On HP decrease:** scan RW regions for u32 values that changed from 0 → positive
   - These are iframe timer addresses that just activated (invincibility frames)
2. **Between damage:** filter candidates to addresses that decrease toward 0
   - The iframe timer counts down between damage events
   - Addresses that stay stuck at high values are eliminated
3. **Confirmation:** when only 1 candidate survives 3+ damage cycles, it's confirmed
4. **Freeze:** write a high value (e.g., 0x7FFFFFFF) every N ticks to keep iframe active

### Platform-Specific Freeze

**File:** `neon-stream/src/scanner.rs` → `GodModeFreezer::tick()`

- **macOS:** `mach_vm_write` via task port (requires debug privileges)
  - Needs `task_for_pid()` + `mach_vm_write()` on the game's task port
  - Currently a placeholder — the `MemoryScanner` already has `task` port access
- **Linux:** `pwrite` to `/proc/<pid>/mem` (requires /proc access)
  - Currently a placeholder — the `MemoryScanner` already has `/proc/<pid>/mem` access

### GodModeConfig (neon-core)
```rust
pub struct GodModeConfig {
    pub enabled: bool,
    pub boss_variant: bool,       // Allow some damage through for bosses
    pub allow_1hp_drops: bool,    // Allow dropping to 1 HP (some bosses check this)
    pub damage_threshold: u32,    // Only block damage above this (boss variant)
}
```

### Integration
Wired to `neon-core/src/combat/godmode.rs` (structs already existed).
The `GodMode` struct processes damage packets and blocks/lets through based on config.

### Implementation Gap

The freeze loop needs to be wired into the main game loop:
```rust
// In neon-stream main loop:
if god_mode.confirmed {
    let freezer = GodModeFreezer::new(confirmed_addr, 0x7FFFFFFF, 5); // freeze every 5 ticks
    if freezer.tick(&scanner) {
        // value was written — god mode active
    }
}
```

## Phase 5: Wire Opcode Mapping

**Status:** ✅ IMPLEMENTED (2026-10-29)

### Inline Hook Probe (v4.1)

**File:** `neon-probe/probe-opcodes.c`

Uses inline hooking on the packet send function at `0x141660360`:

**Flow:**
1. Allocate a code page within ±2GB of the target (for rel32 JMP)
2. Build a springboard (14-byte absolute JMP to hook function) + trampoline (5 saved bytes + rel JMP back)
3. Patch 5 bytes at target (`mov [rsp+8], rcx` → rel32 JMP to springboard)
4. Hook reads internal opcode from packet object at offset +0x434
5. Output: JSONL to `Z:\tmp\neon-probe-opcodes.<pid>.jsonl`

**Output format:**
```json
{"t":12345,"d":"out","op":"0x0190","len":24,"head":"aabbccdd..."}
```

**Build:** `make cross` in `neon-probe/` → produces `probe_opcodes-win.dll`

### Wire Correlation (`wiremap-probe.py`)

**File:** `neon-calib/tools/wiremap-probe.py`

**Algorithm:**
1. Read probe JSONL (internal opcodes with timestamps)
2. Read wire log (wire opcodes with timestamps from `neon-stream --opcode-log`)
3. For each probe entry, find wire entries within `WINDOW_MS` (500ms)
4. Match by exact packet length (most reliable discriminator)
5. Build mapping: wire_op → internal_op

**Usage:**
```bash
# 1. Inject opcode probe DLL
./inject_ud-win.exe --pid <game_pid> --dll probe_opcodes-win.dll

# 2. Run neon-stream with opcode logging
neon-stream --net --opcode-log /tmp/wire.log

# 3. Correlate
python3 wiremap-probe.py /tmp/neon-probe-opcodes.<pid>.jsonl /tmp/wire.log -o wiremap.json
```

**Output:**
```json
{"00db": "00e0", "00e1": "0190", ...}
```

### Per-Connection Scrambling

Wire opcodes are per-connection scrambled. Each new connection (channel change, reconnect)
may have different wire opcodes for the same internal opcodes. The wiremap is built per
connection and cached by the loader.

### Observed Incoming Opcodes (for reference)

From `docs/opcodes.md` (passive capture):
- `0x0013`–`0x0014`: Periodic keepalive-like (16 seen, 0.1/s)
- `0x001b`: 48-byte periodic (5.2s median gap) — server challenge hypothesis
- `0x001d`: 22-byte periodic (147.9s gap) — client answers with 0xfc6b
- `0x005c`: SeqRequest (144/144 matched with 0x00c5)
- `0x0072`: InventoryChange (124 seen, decoded in neon-stream)
- `0x007e`: StatUpdate (1115 seen, verified — bitmask of stats)
- `0x00a7`: PartyLeft (40 seen, verified — char id)
- `0x00ae`: Chat (880 seen, verified — kind byte + text)
- `0x01a9`: MapEnter (16 seen — set-field, map id at u32@36)
- `0x022d`: PlayerSpawn (118 seen, verified — full player data with gear)
- `0x0209`: PlayerLeave (280 seen, verified — char id removal)
- `0x0234`: ShopSign (482 seen — merchant ads)
- `0x022f`: Whisper (13 seen — private messages)

## Phase 6: Packet Injection Features

**Status:** ✅ FRAMEWORK IMPLEMENTED (2026-10-29)

### PacketActuator

**File:** `neon-stream/src/packet_actuator.rs`

The `PacketActuator` connects the `outgoing::PacketBuilder` to the actual TCP socket:

**Architecture:**
```
PacketActuator
  ├── builder: PacketBuilder (constructs packets with IV advancement)
  ├── socket: TcpStream (the game's TCP connection)
  ├── send_cipher: SendCipher (frame encryption state)
  └── current_iv: [u8; 4] (IV for next packet)
```

**Supported features:**

| Feature | Packet | Opcode | Implementation |
|---------|--------|--------|----------------|
| Kami | MoveSelf | 0x00db | Teleport to mob position (x, y) |
| Skill Injection | AttackSkill | 0x00e1 | Skill ID + target OID + target position |
| Full Map Attack | Attack (extended) | 0x00e1 | Extended range marker (0xFFFF) |
| No Delay | Attack | 0x00e1 | Send without cooldown checks |
| Flash Jump | MoveSelf + flags | 0x00db | Flight/dash flag for air jump |
| Mob Disarm | Filter | N/A | Incoming packet filter (placeholder) |

### Encoding

**File:** `neon-stream/src/outgoing.rs`

Added `encode_frame_raw()` for direct socket sends:
- Computes header: `a = ((iv[2] << 8) | iv[3]) ^ 0xdf00`, `c = a ^ swap16(len)`
- Encrypts payload with AES-256-OFB (key from `send::KEY`)
- IV advances via shuffle table after each packet

**IV shuffle table:** 256-byte permutation at MapleStory.exe file offset `0x3bb3d00`.
Verified against 93 real client frames — matches perfectly.

### Wire Opcode Dependency

Actual packet sending requires the wire→internal opcode mapping from Phase 5.
The `PacketActuator` uses internal opcodes; the wiremap converts them to
per-connection wire opcodes before sending.

### Implementation Gap: Socket Discovery

**File:** `neon-stream/src/packet_actuator.rs` → `connect()`

The `connect()` method is a placeholder — it finds game ports via `net::game_ports_of(pid)`
but does not establish a socket connection. The actual implementation needs to:
1. Discover the game's TCP connection to the server (from `/proc/<pid>/net/tcp6` on Linux
   or socket inspection on macOS)
2. Extract the server address (IP + port) from the existing connection
3. Either reuse the existing socket (injected DLL) or establish a parallel connection

```rust
// Current placeholder:
self.socket = None; // ← needs actual socket discovery

// Needed:
// 1. Read /proc/<pid>/net/tcp6 for established connections to game ports
// 2. Parse local_port, remote_ip, remote_port from the connection entry
// 3. Either duplicate the socket or use the injected DLL to write to it
```

## Phase 7: Non-Injection Features (parallel)

**Status:** ✅ FRAMEWORK IMPLEMENTED (2026-10-29)

All feature structs and configs exist in `neon-core`:

| Feature | Module | Status |
|---------|--------|--------|
| Rotation Builder | `combat/rotation.rs` | Struct + targeting modes |
| Auto Char | `progression/` | Framework exists |
| Account Bank | `accounts/` | Struct exists |
| Proxy Profiles | `safety/` | Framework exists |
| Auto Rune | `movement/` | Struct exists |
| Pet Loot/Care | `combat/` | Struct exists |
| Dangerous-Feature Gating | `lib.rs` | `SafetyConfig` with toggles |
| Performance Tweaks | `CONTEXT.md` | Documented |
| Spawn Points | `movement/spawn.rs` | Per-map coordinates |
| Auto Pet Care | `utility/` | Framework exists |

These are **pixel-based** features (already working via `neon-loader` — mouse clicks,
keyboard presses, screen capture). They don't require packet injection.

## Phase 8: Level 30 Warrior

**Status:** 🔄 READY TO RUN (requires Phases 3-6)

### Grinding Plan (from `docs/warrior-10-30.md`)

**Route:**
1. Warrior creation → Job advancement (1st job → 2nd job)
2. Level 10-20: Rain Forest East (Orange Mushrooms)
3. Level 20-25: Tree Dungeon IV (Blue Porings)
4. Level 25-30: Wild Boar Land (Wild Boars)

**Required features:**
- God mode (survive in mob clusters)
- Auto attack + skill injection (fast mob kills)
- Auto potion (HP/MP management)
- Kami teleport (reposition to mobs)
- Loot/pickup (meso collection)

**ETA:** 4-6 hours of gameplay with full automation.

### Character Build (from `docs/features.md`)

`neon-loader --build builds/warrior.json` spends points per level as the meowdb
Warrior guide specifies: STR with DEX at 15, 19-20, 29-30; Power Strike, Slash Blast,
Precise Strikes. Skills on quick slots: Power Strike on Shift, Slash Blast on Del,
Iron Body on End, Recovery on PgUp.

### Configuration
```json
{
  "character": "warrior",
  "route": "warrior-10-30",
  "features": {
    "god_mode": true,
    "auto_attack": true,
    "skill_injection": true,
    "kami": true,
    "auto_potion": true,
    "auto_loot": true
  }
}
```

## Implementation Summary

| Phase | Status | Code Complete | Live Tested |
|-------|--------|:---:|:---:|
| 1: Injector | ✅ Done | ✅ | ✅ |
| 2: Anti-Cheat | ✅ Mock | ✅ | ❌ |
| 3: Session Token | ✅ Hook | ✅ (IAT) | ❌ |
| 4: God Mode | ⚠️ Framework | ⚠️ Freeze is placeholder | ❌ |
| 5: Opcode Probe | ✅ Done | ✅ | ❌ |
| 6: Packet Features | ⚠️ Framework | ⚠️ Socket discovery missing | ❌ |
| 7: Non-injection | ✅ Done | ✅ | ✅ (pixel-based) |
| 8: Level 30 | 🔄 Ready | ❌ Depends on 3-6 | ❌ |

## Next Steps (in order)

1. **Anti-cheat mocks + launcher hook** — `scripts/mock-anticheat.sh install|inject`; live-test both
2. **Live-test Phase 5** — inject `probe_opcodes-win.dll`, capture wire log, run `wiremap-probe.py`
3. **Implement socket discovery** — connect `PacketActuator` to the game's TCP connection
4. **Live-test Phase 6** — send keepalive, kami, skill injection (requires wiremap from step 2)
5. **Implement GodModeFreezer** — wire platform-specific memory write into the main loop
6. **Live-test Phase 4** — stand in mobs, verify HP stays constant
7. **Run level 30 grind** — all features active, monitor via neon-web panel
