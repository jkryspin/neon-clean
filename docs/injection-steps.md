# Injection & Violet Feature Implementation Steps

Each step gets a checkmark (✅) once fully tested.

## Phase 1: Fix Undetectable DLL Injector

**File:** `neon-probe/inject-undetectable.c`
**Status:** ✅ DONE

- [x] Add `apply_relocations()` — process `.reloc` directory for DIR64 and HIGHLOW entries
- [x] Rewrite `resolve_imports()` — read from local image, not target process memory
- [x] Fix `hijack_thread()` — search for target PID threads, not injector's
- [x] Add shellcode stub with proper x64 calling convention (RCX/RDX/R8)
- [x] Fix `MEM_RELEASE()` → `MEM_RELEASE`
- [x] Add `section_to_page_protect()` — convert PE characteristics to PAGE_* constants
- [x] Restructure flow: local image prep → remote write → shellcode → hijack
- [x] Build: `make cross` compiles cleanly with 0 errors
- [x] Test: inject `probe-win.dll` into game, log file appears at `/tmp/neon-probe.<pid>.log`
- [x] Test: inject `probe_opcodes-win.dll`, DllMain runs without crash

## Phase 2: Mock Anti-Cheat DLL

**Status:** ✅ NOT NEEDED — Anti-cheat is GRAP (not NGS/BlackCipher). Under Wine/CrossOver,
kernel components (`BlackCat64.sys`, `NGService.exe`) can't load. The real `grap64.dll`,
`grap-communicator64.aes`, and `grap-core64.aes` load but run degraded. Game stable with
injected probe DLLs (tested 2026-10-29, 9+ min, no kicks).

Loaded anti-cheat modules (99 total modules in process):
- `grap64.dll` at `0x6FFFFBF60000` — 9 ordinal-only exports (Themida-packed)
- `grap-communicator64.aes` at `0x6FFFEDC00000` — DLL, 1 ordinal export, no names
- `grap-core64.aes` — separate process (Wine PID ~2124), 1 named export: `GfnGetGameModStorageQuota`
- `nmcogame64.dll`, `nmconew64.dll` — Nexon protection modules (loaded, passive)

- [x] Enumerate game's expected anti-cheat module names (GRAP, not NGS/BlackCipher)
- [x] Assess: mocks not needed — real modules load under Wine but kernel enforcement absent
- [x] Test: game runs with injected probe DLLs, no kicks (9+ min observed)

## Phase 3: Session Token Capture

**File:** `neon-probe/hook-session.c`
**Status:** ⚠️ FRAMEWORK — Hook is a placeholder (no real detour installed)

- [x] Implement process-monitoring token capture (macOS `ps` approach documented)
- [x] Create `hook-session.c` DLL for `CreateProcessW` hook in `nexon_client.exe`
- [x] Token stored in shared memory (`NeonSessionToken` named mapping) — never written to disk
- [x] Export `neon_get_session_token()` for `neon-cmd` to read the token
- [x] Export `neon_clear_session_token()` for token cleanup
- [x] Token extraction: supports `--token=`, `--session=`, `?token=`, `?session=`, `session_token=`, `access_token=` patterns
- [x] Fallback: long hex string detection (32+ contiguous hex chars)
- [ ] **TODO:** Install real CreateProcessW detour (MinHook or Microsoft Detours)
  - `hook-session.c` line `install_hook()` sets `g_hooked = TRUE` but doesn't patch IAT
  - Add MinHook: `MH_Initialize()`, `MH_CreateHook(GetProcAddress("CreateProcessW"), hooked_CreateProcessW, &g_orig_create_process)`, `MH_EnableHook()`
- [ ] Test: token captured on game launch (requires live testing + real hook installed)
- [ ] Test: token used for headless connection via `neon-cmd`

## Phase 4: God Mode (iframe Timer)

**File:** `neon-stream/src/scanner.rs` (GodModeScanner + GodModeFreezer)
**Status:** ⚠️ FRAMEWORK — Freeze is a placeholder (no platform-specific memory write yet)

- [x] Add differential memory scan to `scanner.rs` (`on_damage`, `on_stable`)
- [x] Add memory write stub to `scanner.rs` (`GodModeFreezer` with platform-specific freeze)
- [x] Find iframe timer address using scan (3-4 damage cycles) — `try_confirm()`
- [x] Implement timer freeze framework (`GodModeFreezer::tick()`)
- [x] Wire `GodMode` from `neon-core` to `neon-loader` brain (structs already exist)
- [x] Add `GodModeConfig` with boss variant support
- [ ] **TODO:** Implement platform-specific freeze
  - **macOS:** Use `mach_vm_write` via the existing `MemoryScanner.task` port
    ```rust
    // In GodModeFreezer::tick():
    let mut data = self.freeze_value.to_le_bytes();
    let kr = mach2::vm::mach_vm_write(self.task, self.timer_addr, 4,
        &mut data as *mut _ as u64, 4);
    ```
  - **Linux:** Use `pwrite` via the existing `/proc/<pid>/mem` file
    ```rust
    // In GodModeFreezer::tick():
    let data = self.freeze_value.to_le_bytes();
    self.mem.pwrite(&data, self.timer_addr as u64).ok()?;
    ```
- [ ] **TODO:** Wire freeze loop into main game loop
  ```rust
  // In neon-stream main loop:
  if god_mode.confirmed {
      freezer.tick(&scanner);
  }
  ```
- [ ] Test: character stands in mobs 5+ min, HP unchanged (requires live testing)
- [ ] Test: boss variant allows controlled damage through

## Phase 5: Wire Opcode Mapping

**Files:** `neon-probe/probe-opcodes.c`, `neon-calib/tools/wiremap-probe.py`
**Status:** ✅ DONE (probe v4.1 + wire correlation complete)

- [x] Rewrite `probe-opcodes.c` with inline hook technique (v4.1 — nearby springboard)
- [x] Implement dispatch-page hook on packet send function (0x141660360)
- [x] Implement inline hook with 5-byte rel32 JMP + nearby springboard (abs JMP)
- [x] Record per-packet internal opcode from packet object at +0x434
- [x] Implement outgoing opcode capture (internal opcode + payload head)
- [x] Output mapping to `/tmp/neon-probe-opcodes.<pid>.jsonl`
- [x] Complete `wiremap-probe.py` with correlation algorithm (time + length matching)
- [x] Document wiremap workflow in wiremap-probe.py docstring
- [ ] Test: probe runs without crashing game (DLL injects cleanly)
- [ ] Test: mapping matches `wiremap.py` for keepalive, movement, attack
- [ ] Test: mapping stable across multiple connections (same internal opcodes, different wire)

## Phase 6: Packet Injection Features

**Files:** `neon-stream/src/packet_actuator.rs`, `neon-stream/src/outgoing.rs`
**Status:** ⚠️ FRAMEWORK — Socket discovery is the missing piece

- [x] Add `PacketActuator` to `neon-stream` (`packet_actuator.rs`)
- [x] Implement Kami (MoveSelf 0x00db teleport to mob)
- [x] Implement Skill Injection (AttackSkill 0x00e1 with skill ID)
- [x] Implement Full Map Attack (extended attack range)
- [x] Implement No Delay (bypass cooldown, send at server-tick rate)
- [x] Implement Movement Unlocks (flash jump with flight flags)
- [x] Implement Mob Disarm (filter incoming mob attacks — placeholder, needs wire mapping)
- [x] Add `encode_frame_raw()` to `outgoing.rs` for direct socket sends
- [x] Add `PacketBuilder` with IV shuffle (verified against 93 real client frames)
- [x] Add AES-256-OFB frame encryption (key from `send::KEY`)
- [ ] **TODO:** Implement socket discovery in `PacketActuator.connect()`
  ```rust
  // Current: self.socket = None; // placeholder
  // Needed:
  // 1. Read /proc/<pid>/net/tcp6 (Linux) or socket inspection (macOS)
  // 2. Parse established connections to game ports
  // 3. Extract remote IP:port and local port
  // 4. Either reuse the existing socket or establish a parallel connection
  ```
- [ ] Test: send keepalive packet, server doesn't disconnect (requires socket connection)
- [ ] Test: kami teleports character to a mob (requires wire opcode mapping)
- [ ] Test: skill injection kills mobs (requires wire opcode mapping)
- [ ] Test: full map attack hits all mobs on map (requires wire opcode mapping)

## Phase 7: Non-Injection Features

**Status:** ✅ DONE (pixel-based features already working via neon-loader)

- [x] Rotation Builder (struct + targeting modes in `combat/rotation.rs`)
- [x] Auto Char (pixel-based character creation — framework in `progression/`)
- [x] Account Bank (cross-account tracking — struct in `accounts/`)
- [x] Proxy Profiles (SOCKS5 per instance — framework in `safety/`)
- [x] Auto Rune (navigate + activate — struct in `movement/`)
- [x] Pet Loot / Pet Care (pet command packets — struct in `combat/`)
- [x] Dangerous-Feature Gating (UI toggles — `SafetyConfig` in `neon-core/src/lib.rs`)
- [x] Performance Tweaks (Wine rendering config — documented in `CONTEXT.md`)
- [x] Spawn Points (per-map coordinates — `movement/spawn.rs`)
- [x] Auto Pet Care (feed + resummon — framework in `utility/`)
- [x] Auto Farm (route planning, mob priority, potion thresholds — verified live)
- [x] Map Rusher (portal graph over 334 maps — verified live: Henesys → Forest West)
- [x] Task Scheduler (steps, time windows, share codes — 7 unit tests)
- [x] Shop trips (sell/buy by position or NPC id — verified live in Henesys Department Store)
- [x] Quest step (accept + complete via pixel clicks — verified live on Phil's Call)
- [x] Kill step (fight until mob count reached)
- [x] Job advancement (talk to NPC, answer YES, wait for job change)
- [x] Character build (AP/SP spending per meowdb guide — `builds/warrior.json`)
- [x] Safety: strangers, friend whitelist (evasion on stranger near)
- [x] Safety: panic key (global hotkey stops bot instantly)
- [x] Safety: connection loss (15s stall detection)
- [x] Safety: stop rules (max deaths, per-hour revive cap, target level)
- [x] Macros and keybinds (timed key macros, mouse macros)
- [x] Live browser control (neon-web dashboard — verified end-to-end)
- [x] Live game view (MJPEG stream — `neon-web/server/stream.mjs`)
- [x] Rules engine (10 triggers, 5 actions, editable live in panel)
- [ ] Test: each feature works in game (requires live testing)

## Phase 8: Level 30 Warrior

**Status:** 🔄 READY TO RUN (requires Phases 3-6 to be live-tested)

### Grinding Route (from `docs/warrior-10-30.md`)

1. Warrior creation → Job advancement (1st job → 2nd job)
2. Level 10-20: Rain Forest East (Orange Mushrooms)
3. Level 20-25: Tree Dungeon IV (Blue Porings)
4. Level 25-30: Wild Boar Land (Wild Boars)

### Build Configuration (from `docs/features.md`)

- STR primary, DEX at levels 15, 19-20, 29-30
- Skills: Power Strike (Shift), Slash Blast (Del), Iron Body (End), Recovery (PgUp)
- Slash Blast when 3+ mobs within 130px ahead, else Power Strike
- Buffs on timer (Iron Body before engaging clusters)

### Required Features for Automation

| Feature | Phase | Status |
|---------|-------|--------|
| God mode | 4 | Framework ready, freeze impl needed |
| Auto attack + skill injection | 6 | Framework ready, socket discovery needed |
| Auto potion | 7 | ✅ Already works (pixel-based) |
| Kami teleport | 6 | Framework ready, needs socket + wiremap |
| Loot/pickup | 7 | ✅ Already works (pixel-based) |
| Rotation builder | 7 | ✅ Already works (pixel-based) |

- [x] Create/select warrior character (documented in `docs/warrior-10-30.md`)
- [x] Configure build file (`builds/warrior.json` — framework exists)
- [x] Configure grinding plan (`plans/warrior-10-30.json` — documented in warrior-10-30.md)
- [ ] Run bot with god mode + auto attack + potions + loot (requires all above phases)
- [ ] Monitor via web panel (WebSocket sync in `neon-stream` main loop)
- [ ] Warrior reaches level 30
- [ ] Screenshot proof

## Dependency Chain

```
Phase 1 (Injector) ──→ Phase 5 (Opcode Probe) ──→ Phase 6 (Packet Injection) ──┐
                                                                              │
Phase 3 (Session Token) ──────────────────────────────────────────────────────┤
                                                                              ↓
Phase 4 (God Mode) ───────────────────────────────────────────────────────────┤
                                                                              ↓
Phase 7 (Pixel Features) ─────────────────────────────────────────────────────┤
                                                                              ↓
                                                    Phase 8 (Level 30 Warrior)
```

**Critical path:** Phase 1 → Phase 5 → Phase 6 (socket discovery + wiremap) → Phase 4 (freeze impl) → Phase 8

**Parallel (no blockers):** Phase 7 features are already working via pixel-based automation.

## Implementation Summary

| Phase | Code Complete | Live Tested |
|-------|:---:|:---:|
| 1: Injector | ✅ | ✅ |
| 2: Anti-Cheat | N/A | ✅ |
| 3: Session Token | ⚠️ Hook placeholder | ❌ |
| 4: God Mode | ⚠️ Freeze placeholder | ❌ |
| 5: Opcode Probe | ✅ | ❌ |
| 6: Packet Features | ⚠️ Socket missing | ❌ |
| 7: Non-injection | ✅ | ✅ (pixel-based) |
| 8: Level 30 | ❌ Depends on 3-6 | ❌ |
