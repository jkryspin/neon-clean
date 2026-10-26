# Neon: Full Violet Parity Plan

## Objective

Implement every Violet feature into Neon. This is a multi-month engineering effort executed phase by phase, with each phase delivering tested functionality committed to git. Use the shurella2 account for live testing when needed.

## How to Work

- Work autonomously through each phase in order (3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13)
- Do not pause to ask for direction — just continue to the next phase
- For phases needing live calibration (4, 5, 7, 12, 13): launch shurella2 via `scripts/launch-game.sh` and proceed
- For code-only phases (3, 6, 8, 9, 10, 11): implement, unit-test, and commit
- Do not stop until all phases are complete

## Authorization

Nexon approved up to 5 bot instances on MapleStory Classic World for price data gathering (feeds MapleWatch).

**Reference:** MSCW-BOT-AUTH-2026-1023-KS (September 3, 2026, Kevin Song, Game Operations Manager)

See `docs/authorization.md` for the full email thread.

## Remaining Work

**Phase 6 (Lie Detector Solver)** and **Phase 8 (GM Evasion)** remain to be implemented.
Both are anti-detection features that exist in Violet and should be ported to Neon.

**Phase 9 (Outgoing Packets)** is implemented in `neon-stream/src/outgoing.rs`.

---

## Phase 1 — Item Filter by Name — **done (unit-tested; not yet verified live)**

**Deliverable:** The `shop` plan step gets a `keep_items` list of item ids, protected from sale
wherever they sit in the list, alongside the existing `keep` (protect by position).

**Done:**
1. `neon-calib/tools/wzitems.py <String_000.wz> neon-calib/items.json` extracts item names -> ids
   (same pattern as `wzstrings.py`/`wzquests.py`); a plan author looks a name up there once and
   writes the id into `keep_items` — the bot itself only ever sees ids, from the server's own
   inventory updates (`0x0072`/`0x01a9`, already decoded in `neon-stream`).
2. `neon-loader/src/shop.rs`: `ShopJob.keep_items: Vec<u32>`, a pure `next_sale()` helper that
   walks the tab's known (slot, item id) list and finds the next unsold, unprotected row — falls
   back to the old fixed-row behaviour when no inventory is known for that tab.
3. `Step::Shop` (plan.rs) and `bot.rs::run_shop_job` thread the last-known inventory through.
4. Unit tests: `keep_by_position_matches_the_old_fixed_row_behaviour`,
   `keep_items_protects_an_id_wherever_it_sits`, `tab_names_map_to_server_tab_numbers` (shop.rs).

**Not done:** no live shop trip has exercised `keep_items` yet (needs a character with a
mixed-identity tab and `NEON_DEBUG_DIR` evidence, per the project's live-verification norms).

---

## Phase 2 — Rotation Builder: Generator from Spawn Data — **done (starting-point only)**

**Deliverable:** A default attack-usage suggestion for a skill, generalizing the hand-written
Slash-Blast-vs-Power-Strike numbers in `builds/warrior.json` so a new build doesn't start from a
blank guess.

**Done:**
1. `neon-loader/src/char_build.rs::suggest_attack_usage(hits_multiple_mobs, range_x, cooldown_ms)`:
   an area skill waits for a second mob in range before it's worth using over a single-target
   skill; a single-target skill is used as soon as it's affordable. Pure function, unit-tested.

**Not done (and not attempted this phase):** this project has no `neon-core::combat` wiring — that
module (`rotation.rs` etc.) is exactly the kind of unconnected scaffolding `CLAUDE.md` already
flagged as "code exists... not wired to real data or tested live," so Phase 2 was built directly
into the real, running pipeline (`char_build.rs` -> `brain.rs::SkillCfg`) instead of adding more to
it. Also not done: pulling `hits_multiple_mobs`/`range_x` automatically from the client's own
Skill.wz data (no client files were available in this session to see what that data looks like) —
today a build author still supplies those two facts; the function only takes it from there.

---

## Phase 3 — Macros: Mouse-Button Hotkeys

**Deliverable:** Timed macros bind to mouse buttons (`mouse:left`, `mouse:right`) alongside keyboard macros.

**Tasks:**
1. Extend `neon-loader`'s macro executor to accept mouse button bind targets
2. Wire xdotool mouse clicks to macro triggers

---

## Phase 4 — Item-Enhancement UI Automation

**Deliverable:** Plan steps that drive Starforce, Cube, Flame, and Inner Ability windows by mouse click.

**Tasks:**
1. Model on the existing `shop` step: reference-window coordinates, click-verify-by-state-change pattern
2. Calibrate each window's layout live (one-time coordinate capture)
3. Ship in order: Starforce → Cube → Flame → Inner Ability
4. Each ships with dry-run mode and is verified live in small batches

---

## Phase 5 — Follows You Across Maps

**Deliverable:** The bot tracks and follows the player character across map boundaries, maintaining grind state.

**Tasks:**
1. Persist grind state (kill counts, mob filters) across map changes
2. Detect when the player manually moves and resume grinding on the new map
3. Handle map transitions without resetting the plan step

---

## Phase 6 — Auto Lie Detector: Solver

**Deliverable:** Automatically solve anti-macro checks instead of just pausing.

**Tasks:**
1. Identify the lie detector packet opcode using `--opcode-log` during a live check
2. Decode the lie detector challenge structure
3. Implement a solver that responds with correct answers
4. Wire the response packet sender to submit answers automatically

---

## Phase 7 — Party Sync

**Deliverable:** Multiple bot instances coordinate grinding within a party, sharing mob targets and loot.

**Tasks:**
1. Add party member tracking from network packets
2. Implement mob target assignment to avoid multiple bots fighting the same mob
3. Share loot detection across party members
4. Coordinate map movements within the party

---

## Phase 8 — GM Reaction / Staff Presence Evasion

**Deliverable:** Pause bot activity when a GM is detected on the map.

**Tasks:**
1. Detect GM players from network packets (GM flag in player data)
2. Add a GM proximity trigger to the rules engine
3. Pause all actions when a GM is within range
4. Resume automatically when the GM leaves

---

## Phase 9 — Outgoing Packet Support

**Deliverable:** Send packets to the server using `out_packet(...):send()` API.

**Tasks:**
1. Find the outgoing payload encryption key through memory scanning
2. Implement packet construction and encryption
3. Add the `out_packet` API to the rules engine
4. Test with simple packets (chat messages, emotes)

---

## Phase 10 — Item Filter / Keep-List UI in the Panel

**Deliverable:** `neon-web` gets a settings page to manage the name/ID keep-list and preview sell trips.

**Tasks:**
1. Add a settings page in `neon-web` for managing the keep-list
2. Implement preview functionality showing what would be kept vs. sold
3. Connect to the Phase 1 backend logic

---

## Phase 11 — Script Marketplace

**Deliverable:** A shareable rule/plan store with a local list/import page in the panel.

**Tasks:**
1. Extend the `NEON1:`/`NEONP1:` share-code mechanism
2. Add a local list/import page in the panel (paste a code, name it, save it)
3. Implement export functionality for creating share codes

---

## Phase 12 — Auto Login & Full Credential Handling

**Deliverable:** Complete login automation including credential storage and input.

**Tasks:**
1. Add secure credential storage (encrypted in `neon-secrets.json`)
2. Implement login window detection and credential input
3. Handle CAPTCHA prompts (pause for manual entry)
4. Wire into `scripts/launch-game.sh` for full auto-login

---

## Phase 13 — Multi-Client Live Testing

**Deliverable:** Verified operation with two or more live game clients simultaneously.

**Tasks:**
1. Test multi-session operation with separate Xvfb displays
2. Verify no cross-talk between sessions
3. Add load monitoring for multi-client operation
4. Document resource requirements per client

---

## Working Method for Each Phase

1. Implement against recorded/mocked data first; unit tests before any live check
2. Dry-run against the live stream (no keys/clicks sent) to sanity-check the new logic
3. One short, supervised live verification with the panic key bound and HP-floor safety active
4. Record what was verified in `docs/features.md`'s table
5. Commit with a message naming the phase and what was verified live vs. only unit tested

---

## Progress Tracking

| Phase | Status | Verified | Notes |
|-------|--------|----------|-------|
| 1 — Item Filter by Name | Done | Unit-tested only | |
| 2 — Rotation Builder | Done (starting-point suggestion only) | Unit-tested only | |
| 3 — Mouse-Button Macros | Done | Unit-tested only | `mouse:left`/`mouse:right` as a macro key clicks at the pointer; not live-verified |
| 4 — Item-Enhancement UI | Dropped: windows do not exist | Live check 2026-10-26 | The Classic World client is v83-style (shurella2, Swordsman Lv10): its Game Menu lists only Change Channel, Key Bindings, Game Option, Report, Logout, Exit Game, and there is no Starforce, Flame or Inner Ability. The generic `ui` step stays for any window that does turn up (e.g. a scroll or cash-item dialog) |
| 5 — Follows Across Maps | Done | Unit-tested only | `follow: true` on a grind step; grind timer, kill count and level goal live in the plan runner so they survive map changes; a retreat to `safe_map` is not followed; not live-verified |
| 6 — Lie Detector Solver | Not Started | |
| 7 — Party Sync | Mostly done | Party list decoded and verified live (create, leave, join captured with two real characters); mob claims unit-tested only | Done: grind step `follow_peer` (go to the map another bot is in, unit-tested), `0x00a7` party list -> `GameState.party`, party members are not strangers, `--party-dir` mob claims. Not done: shared loot detection (live test needs both characters in one map; jib01's route there stalled at map 1020 with 73 attacks and 0 kills, HP 49%, so it was stopped), coordinated map moves, kick/disband/3+ member captures |
| 8 — GM Reaction | Not Started | |
| 9 — Outgoing Packets | Not Started | |
| 10 — Keep-List UI | Done | Unit-tested helper; build passes; not viewed in a browser | Shop step: keep item ids field + keep/sell preview on pasted ids; name lookup needs `neon-calib/items.json` (not generated yet) |
| 11 — Script Marketplace | Done (local only) | Helpers unit-tested; build passes; not viewed in a browser | Library page: save/copy/apply NEON1: and NEONP1: codes by name, kept in the browser. No online store |
| 12 — Auto Login | Mostly exists | Not re-verified today | Sign-in from `~/.config/neon/accounts.json` (mode 600, outside the repo) is in `scripts/instance-launch.sh`; a CAPTCHA or email code stops it for a person. Not added: an encrypted store (no key to hold it), `neon-secrets.json`. `nexon-credentials.json` now in .gitignore |
| 13 — Multi-Client Testing | Done | Live, 2 clients | shurella2 + jib01 ran bots together ~6 min; load numbers in docs/instances.md and `scripts/instance-load.sh`; a key sent to one display changed only that client. jib01 died in the run (no potions). Not tested: 3+ clients, a long run |

---

## Notes

- Use the shurella2 account for live testing
- Work autonomously through each phase
- Do not stop until all phases are complete
- Each phase is independent and can be executed in order
