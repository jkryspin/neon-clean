# Mapping opcodes: the workflow

Goal: every opcode, incoming and outgoing, described in `neon-protocol/opcodes.toml` with evidence, and visible in
**Neon View** (`neon-view/`), which is also how replays and the opcode explorer work. This page is how a packet goes from
"observed" to "verified", and what has been done so far.

## The three artefacts

| Artefact | What it is |
|---|---|
| `neon-protocol/opcodes.toml` | The registry. One or more entries per (direction, opcode): name (only with evidence), status, size bounds, fields, **variants** (chosen by a mode byte), **records** (`header + n x stride + tail`, optionally checked against a count byte). Validated on load; every `verified` variant must decode a real captured packet as itself (test `evidence_rule_...`). |
| `*.nsr` session records | JSON lines, one packet per line, both directions, plaintext: `{"t":ms,"d":"in"\|"out","hex":"..."}`. Older stamped incoming logs replay too. Real captures stay local (`.gitignore`: `*.nsr`, `run/`); small test fixtures are committed on purpose (`*.jsonl` / fixture JSON) after checking they carry no tokens or names. |
| **Neon View** | `scripts/view.sh NAME` (live) or `neon-view --log file` (replay with play/pause/speed/seek). The page shows the world, the packet inspector, the **opcode explorer** (byte map: explained / id reference / constant / unexplained) and a **"map next"** list ranked by unexplained traffic. |

## Capturing

* **Incoming, passive (safe):** (since 2026-10-26 evening the log carries each packet's real capture time from tcpdump, not the time it was printed; older logs are quantised to ~1 s, so request/reply delays measured from them have about a second of slack)
   `scripts/view.sh INSTANCE` runs `neon-stream --net` through `tcpdump` into `run/view.INSTANCE.log`.
  Nothing touches the game process.
* **Outgoing, short attach (opt-in):** the opcode and plaintext are read from the client's packet object just before encryption,
  not by breaking the cipher. `neon-calib/tools/outraw.py` (via `sudo gdb -p PID`, about 12 s) logs the raw object and the wire
  length of each send; `neon-calib/tools/outbuild.py raw.jsonl out.nsr` turns it into exact plaintext (length at `+0x428`,
  validated 41/41 against the sends). **Treat every attach as able to cost the session.** Tally for 2026-10-26 (short attaches of 12-34 s each, taken with a few minutes between them): 2 of 5 were followed, a few minutes later, by a disconnect to the login screen ("Unable to connect to the game server"; one on a test account after a 34 s attach, one on the main account after a 20 s attach), and the very first, much longer sessions (60-150 s) dropped both test clients. The memory reader still showed the character as alive right after the capture each time, so the drop is delayed. Recovery works (`scripts/instance-launch.sh NAME`, one attempt, then back off) but costs a minute or two and some risk. So: test accounts only, one capture per action set, keep the numbers of attaches low, never unattended, and prefer re-using the committed fixtures over capturing again. Addresses are specific to Classic World v1.0.1.

## Finding structure (tools, in the order they pay off)

| Tool | Question it answers | Example result |
|---|---|---|
| Explorer (in the page, `/explore`, `/todo`) | Which bytes vary, which are constant, which hold known ids, what is still unexplained? | ranked `PlayerMove` first, so movement became the next job |
| `neon-calib/tools/recfind.py` | Is it `header + count byte + n fixed records`? (the count byte is a checksum chance cannot satisfy) | rediscovered MobMove/PlayerMove, found PetMove and NpcMove |
| `neon-calib/tools/idscan.py` | Which fields hold ids of mobs / players / drops we already know? | attack broadcasts: player at `@2`, mob at `@63` |
| `scripts/registry-refs.py LOG` | Write what `idscan` found into the registry (fields `char_id` / `mob_oid` on still-unexplored entries; never touches verified ones) | `0x02b0`, `0x02b7`, `0x02ba`, `0x02bd`, `0x047b` reference players |
| `neon-protocol/examples/validate.rs` | Does this registry file pass validation? `cargo run --example validate -- path` | catches a script writing an out-of-bounds field |
| `neon-calib/tools/lifecycle.py` | Does a packet mark the start or end of an entity's presence (its movement packets before vs after, against random times)? | found `PlayerLeave 0x0209`; `0x02bb` is arrival-like; re-finds mob kill/hit and `PetSpawn` |
| `scripts/correlate.py LOG...` (2026-10-27) | **Run this first, systematically, over every still-open opcode** rather than picking one by eye: for each opcode's best id-bearing field, does it line up in time with a mob kill (`0x03d4`), PlayerSpawn (`0x022d`), or PlayerLeave (`0x0209`) for that SAME id, at least half the time? Generalizes the one-off checks already done by hand for `0x0441` and the `0x0279` family into a repeatable sweep. Report only, writes nothing; a hit is a LEAD, not a proof - always follow up with the direction check (strictly before/after, like `0x02b3`'s) before trusting it, since a merely-frequent packet can look correlated with a landmark by base-rate alone (`0x02b7` looked like a leave-triggered broadcast at 83% co-occurrence, but a mixed before/after split showed it's not). | first run found `0x03e9` (100% near a kill, same strength as `0x0441`) and `0x02b3` (confirmed a likely sixth member of the post-spawn `0x0279` family); second run (after adding the leave landmark) found `0x02b7` looked promising but didn't hold up, and incidentally flagged `0x022e` as a false "new" candidate purely because its already-thorough hand analysis didn't use phrasing the tool's crude text-match recognized - broadened that match instead of re-deriving anything |
| `neon-calib/tools/poscorr.py` | Does a field hold the position of the entity the packet refers to? | attacker position pairs in `0x02a8` (77% within 30 px) |
| `neon-calib/tools/actlab.py` | Which opcodes does one isolated action produce (baseline + repeats)? | party create/leave |
| Map geometry (`neon-calib/maps/<id>.json`) | Do decoded positions lie on real platforms / at real NPC positions? | spawns 383/384, NPC moves vs the NPC list |
| **Marks** + `neon-calib/tools/marklab.py MARKS LOG...` | Which opcodes follow a hand-made action? Type a label into the page's Mark box (stored in `run/marks.jsonl`, `POST /mark?label=`) when you do something in the game; `marklab.py run/marks.jsonl run/sessions/X.log` lists, per mark, the opcodes whose count in the next 15 s is far above their background rate. Repeat the action: an opcode that follows every repeat is the candidate (then confirm with the evidence rules). | needs logs with real capture times |
| Aligned outgoing capture | Which sent packet belongs to which key press? | attack `0x00e1` at each `ctrl`; movement `0x00db` |

**Marks pitfall (seen 2026-10-26):** never run an action loop whose period is a round number (60 s) without checking the periodicity first: `periodic.py` showed `0x004f` fires every 60 s, which phase-locked with a 60 s test loop and looked like "3/3 repeats" of opening the stats window. Vary the gaps (random 5-20 s) and run `periodic.py` on candidates before trusting them.

**Corrected result (2026-10-26 19:15):** with the windows actually opened (screenshot-checked; the menu entries of `i` clicked at (1040, 503/529/555/581/633) on the 1600x1000 display), opening and closing the item, equipment, stats, skills and quest windows 4 times each produced no repeating packet in either direction.

**NPC dialog (2026-10-26 19:30, jib01 next to the quest NPC Pio in Amherst):** clicking the NPC opened the dialog 5 of 5 times (pixel-checked) and END CHAT closed it; neither produced a repeating incoming or client->server packet (only the usual background: pings, keepalives, NPC move echoes). Opening and closing an NPC dialog is local to the client; what goes to the server happens on the buttons (NEXT, accept), not tested to avoid changing the test account's quests.

**Party window (2026-10-26 20:00, jib02):** the SHORT CUT button (1043, 775) then PARTY (1030, 607) opens the party list; four clicks on CREATE (295, 593) and LEAVE (353, 641), and further single clicks at several heights, created no party (the list stayed empty) and produced no packets, so no wire/internal pair came out of it. The button probably needs a different click point or a condition I did not find; party create/leave stays an open pair for the numbering fit.

**Retracted (2026-10-26 19:00):** an earlier note here said the inventory, equipment, stats, skills and quest windows are client-only because `i e s k q` produced no packets. That test was invalid: on this client `i` opens a "SHORT CUT" menu (ITEM, EQUIPMENT, CHAR.STATS, SKILLS, PARTY, QUEST, CRAFTING, MAPLE CHAT, MAPLE SOCIAL, MAILBOX), and the other keys did not open the windows at all. Always screenshot (`import -window root`) to confirm that an action did what you think before reading its packets.

**Negative result 2:** after walking jib01 next to the Amherst mailbox, 4 clicks on it (random 6-15 s gaps) opened no window and produced no repeating opcode (candidates appeared in at most 1 of 4 windows and were ordinary town traffic). Walking by held arrow key (`xdotool keydown Left`, 3.2 s) works on the test account.

**Contamination check (added 2026-10-26, after a real incident):** `scripts/registry-facts.py` trusts whatever log files it is given.
A disconnected account's `neon-stream` can keep running and its connection flap, falling back to the old per-block heuristic, which
silently produces garbage packets under MANY unrelated opcodes at once (not just one) - this happened once (jib02, 5861 garbage
packets) and inflated or invented 23 registry entries before it was caught. **After every `registry-facts.py` run**, rebuild an
independent pool from the same file list and check each FACTS sentence's claimed packet count against it; an entry whose count is
less than half supported by the fresh pool is contamination, not real traffic - trace it to the source file and exclude it.

**Live verification (2026-10-26 22:26):** rebuilt and restarted both views after the `neon-world` Whisper/ShopSign/Reactor event work; confirmed live on real traffic within a minute: the Chat panel showed real shop-ad lines (`[shop 368615] Health Potion`), and the Feed panel showed a complete organic reactor lifecycle (`reactor 44350637 hit (1)` -> `(2)` -> `(3)` -> `destroyed`) from a passerby. Remember: a long-running `scripts/view.sh` process keeps whatever registry/decoder was compiled in when it started - rebuild (`cargo build --release`, `touch`ing a source file first if cargo's cache looks stale) and restart the view to see new registry/decode changes.

## Evidence rules (what gets a name)

1. A **name** needs an independent check, not a plausible story: a known value from another source (memory reader, map file, EXP counter),
   an isolated action that reproduces it, or a pairing/ id match that chance does not explain. Record the numbers in the entry's `summary`.
2. `verified` additionally needs a **real packet as a test fixture** that decodes as that variant. The registry test fails otherwise.
3. Guesses stay as `structure` with a summary that says "unverified"; wrong guesses are corrected in place (`0x02fe` was first read as
   movement, `0x0456` as ids; both were wrong and the entries say so).
4. Fields carry **bounds** (`min`/`max`/`charset`) so decoding noise (the incoming cipher key is only estimated, see the protocol notes) is rejected.

## Status (2026-10-26 overnight, for the 6am ET check-in)

Registry (refreshed 2026-10-26 ~22:50): **159 incoming opcodes** (41 name slots filled - MobMove/PetMove/MoveSelf each cover two
length-variant forms of the same event, not a collision - 22 at the best `verified`, 7 `decoded`, 38 `structure`, 92 bare
`observed`) and **34 outgoing entries** (11 named; outgoing entries are the client's INTERNAL numbering - see
`docs/network-protocol.md` for the separate, per-connection WIRE numbering, which is the open problem for Stage B, not a
registry concern). Both the receive and send ciphers are exact (not heuristic) as of the `ab800c1`/`3b8f816`-era work; a
capture that starts at a connection's hello decodes every frame, including the first 12.

Named incoming: AvatarModified, Chat, DropRemove, DropSpawnItem, DropSpawnMeso, ExpMessage, InventoryChange, LoginInventory,
MapEnter, MesoGainMessage, MobControl, MobHit, MobKill, MobMove, MobSpawn, MoveMobAck, NpcMove, PartyCreated, PartyJoined,
PartyLeft, PetMove, PetSpawn, PickupMessage, Ping, PlayerAttack, PlayerLeave, PlayerMove, PlayerSpawn, QuestMessage,
QuestRecordMessage, ReactorCredit, ReactorDestroyed, ReactorHit, ReactorSpawn, SeqRequest, ShopSign, StatUpdate, Whisper.
Named outgoing: Attack, AttackSkill, KeepAlive, MoveMob, MoveSelf, PartyCreateRequest, PartyLeaveRequest, PickupDrop, SeqReply, UiAction.

### Overnight additions (2026-10-26 21:00-22:00, active mapping on jib01/jib02, the user's permission to go active or online)

| Finding | Evidence |
|---|---|
| `Whisper` (`0x022f`) | char_id references a real player in 5/5 senders; four length-prefixed strings in a row (name, message, name again, message again, byte-for-byte identical) in 60/60 real packets; casual short text ("cc plz", "ur a bot") unlike `0x00ae`'s trade-shout format |
| `ShopSign` (`0x023d`) | 13 distinct senders (standing shop owners) account for all 1611 text packets; mode byte discriminates silent (1601/1601 at mode 0) vs text (mode 3); real ad text verbatim ("drops, equip, cheap", "Health Potion") |
| `AvatarModified` (`0x02b8`) | char_id matches 24 of 27 distinct senders; carries the SAME `[slot u8][item id u32]`-terminated-by-0xff equip list pattern as `PlayerSpawn`, for many different players, not a login one-off |
| `ReactorHit`/`ReactorSpawn`/`ReactorDestroyed` (`0x047b`/`0x0487`/`0x048f`) | one Amherst quest crate, hit 3 times with Ctrl, broke and dropped the exact 2 quest items + mesos; the hit-counter byte went 1,2,3 across the three `0x047b`, matching the 3 real hits, oid constant throughout, `0x0487` before and `0x048f` right after |
| `MesoGainMessage` (`0x0085`) | one clean observation: picked up a 9-meso ground drop, 0.16 s later a 6-byte packet with u32=9 |
| `0x02a8`/`0x02a9`/`0x02aa` target-record `damage` field | summed over a mob's attacks, it is 0.7-1.6x the mob's (independently verified) spawn HP in 71%/56%/49% of kills; every other offset tried gives ~0% |
| Chat (`0x00ae`) kind 2 trailer | matches `Whisper`'s schema exactly (name+message duplicated, same constants) - a buddy/party-adjacent shout, not the plain public kind 3/8 |
| Climbing a rope/ladder | confirmed via the memory reader (grounded: false, y goes negative, x pinned to the ladder's recorded x); the `MoveSelf` fragment layout that explains ordinary walking does NOT fit the climbing frames - not solved, flagged as the next concrete step |
| `scripts/registry-facts.py` contamination (self-found and fixed) | a disconnected account's (jib02) `neon-stream` kept running and its flapping connection fell back to the old heuristic, producing 5861 garbage packets that inflated or invented 23 registry entries in the first automated FACTS pass; all 23 traced to 0% support in verified exact-decoded data and were reverted/removed; `registry-facts.py` now auto-promotes `observed` to `structure` with a field when a single id-reference is strong (>=80%, >=5 hits) |

### Later overnight additions (2026-10-26 22:30-23:00, passive-only - organic multiplayer traffic, no further live actions)

| Finding | Evidence |
|---|---|
| A post-spawn status family: `0x0279`/`0x02bb`/`0x02bd`/`0x0281`(`PetSpawn`) | same-char_id, time-window check over 2474 `PlayerSpawn` events: 65.9%/55.0%/12.6%/26.9% of spawns get one of these for that SAME player, always at or after the spawn (never before); looks like optional per-player state sent right after a player becomes visible (buffs, a rarer flag, pet ownership) |
| `0x022e` is a lighter, temporary drop from render range | NOT the same event as the confirmed `0x0209` `PlayerLeave`: that player's own packet gap jumps from a 0 s median (as active as baseline) to a 16.8 s median right at this event, and exceeds 10 s in 65.0% of cases vs 0.9% baseline (795029-gap baseline, 1759 events) - weaker/shorter than 0x0209's near-total silence + 76/275 re-spawn pattern, so it got a careful unnamed note instead of reusing that name (briefly double-named it `PlayerLeave` by mistake and caught it via a duplicate in the live `/opcodes` listing) |
| `0x00ae` `Chat` kind 32 is not chat text | bigger pool (290 packets): the "text" field is always a well-formed player name, not "Name: msg"; a u32 nine bytes after a 100%-constant `0x004faed4` marker takes only 7 distinct values across all 290 packets, every one shaped like a classic equip id - a name-keyed per-player broadcast carrying one item id, not the coincidental "IP fragment" guess from the first look |
| Self-inflicted build break, found and fixed | a prior `git add -A neon-protocol docs` picked up a stray empty `neon-protocol/src/main.rs` scaffold file (never referenced by anything) sitting in that directory and committed it, which broke `cargo build` for the whole crate at HEAD; `scripts/test-headless.sh` caught it immediately on the next run and it was removed in the following commit |
| `neon-view` improvements (the jib02-contamination root cause, closed) | a live page never showed `World::reactors` (a Reactors panel next to Mobs/Drops), never summarized registry coverage (a header stat: opcodes seen this session by verified/decoded/structure/observed), and never color-coded `Chat` by kind (whisper/shop now stand out). More importantly: `neon-stream` now marks a heuristic-decoded line distinctly (`len N heuristic:` instead of `len N:`), which every existing pool-building regex already excludes automatically (confirmed against `registry-facts.py`'s own pattern); `neon-world::parse_log_line` rejects those lines outright, and a new `Snapshot::heuristic_skipped` counter plus a red header warning in `neon-view` means a flapping/desynced connection (exactly what jib02 did, silently, before tonight) is now visible within seconds instead of discovered after the fact by auditing a contaminated pool |

### Stage B (approved by the user, 2026-10-26; recorded in `docs/authorization.md`)

Approval covers headless packet injection, test server NP12-T first, then NP12; exponential reconnect backoff; no tokens in
repo/logs; the Sunday maintenance pause; all earlier rate/timing terms. Work so far stayed **entirely offline**: `neon-stream/src/send.rs::encode_frame`
builds a correct client->server frame (header, IV chain, AES-256-OFB) and reproduces two real captured frames byte for byte.
**Blocker found and not yet solved:** the per-connection wire opcode numbering (the number a packet carries on the wire differs
from the client's internal opcode, and differs again on every connection: four connections checked, four different keepalive
numbers). It is not readable from the wire in the clear, not in the client's process memory (checked by scanning ~2 GB for an
indexed table), and no simple function (affine map, xor-multiply, 16-bit linear generator, or 3,840 hash constructions over
IVs/opcode/a connection's opaque post-hello blob) reproduces it. Four short debugger traces of the plain (non-virtualised) send
path on jib02 narrowed where the rewrite happens (a buffer store at position 0, value known in one trace) but not who calls it;
**jib02 was disconnected after its 6th attach in 40 minutes** (the same ~1-in-3 risk measured earlier in the session) and has
not been relaunched. No further attaches were made. Until the numbering is solved against recorded connections, nothing
connects to NP12-T or sends anything live; this is the one item genuinely blocking "all the mapping" from being complete.

### What the evidence looked like (so it can be repeated)

| Finding | Evidence |
|---|---|
| Movement (`PlayerMove`, `MobMove`, `PetMove`, `NpcMove`) = header + n x 21-byte fragments | count byte == n in 4132/4132, 5477/5477 packets; fragment ends on map platforms 96-100%; NPC starts match the map file's NPC positions |
| `PlayerAttack` header: attacker position twice, per-attacker trait @6 | poscorr: 77% within 30 px; @6 equals the attacker's usual value in 99% of 1487 |
| `ExpMessage` amount | equals the EXP-counter delta in 26/26 kills |
| `MobSpawn` HP @225 | equals the public database HP for the template in 100% of 540 packets |
| `PlayerSpawn` level @14, equipment list | beginner town 5-11 vs hunting map 11-49; slot -> item category consistent in 55/55 spawns |
| `DropSpawn` item id / mesos | the six picked-up item ids are exactly the ids in the drops |
| Outgoing `MoveSelf`, `Attack` | aligned with logged key presses; last position equals the memory reader's |
| Outgoing `PickupDrop` | 6/6 oids are spawned drops; each answered by `DropRemove` (reason 2, picker = us) in 0.6-1.2 s |
| Outgoing `PartyCreateRequest`/`PartyLeaveRequest` | byte-identical on repeat; incoming `PartyCreated`/`PartyLeft` follows in 0.2-1 s |
| Attack target record +8 = damage (`0x02a8`/`a9`/`aa`) | summed over attacks on killed mobs it is 0.7-1.6x the spawn HP in 71% of 2286 kills (56% / 49% for the variants); offsets +0, +4, +12, +16, +20: 0%; real-kill fixture `damage_kill.json` |
| `MoveMob` and `MobControl` | outgoing mob moves are only for mobs whose `0x03d5` carried our id (5/5); all 26 `0x03d5` are for already-announced mobs |

### Corrections made along the way (kept visible on purpose)

* `0x003d`, `0x002c`, `0x006e`, `0x023f` ("1 s ticks", counters with constant `fe`/`fc` tails) never existed: they were frames the old per-block heuristic decrypted with the wrong constant. After exact decryption (docs/network-protocol.md, 2026-10-26 18:40) none of them occurs in 1,600 packets (they were 900 in 30 min). The registry entries are removed. Treat any entry derived from logs written before commit `ab800c1` as unconfirmed until it reappears in an exact-decoded log.
  **2026-10-27 recurrence:** `scripts/registry-facts.py`'s "skip the first 13 lines of each file" IV-sync heuristic was not enough - a capture file can flap/reconnect mid-file, re-triggering the heuristic well past line 13, with no way to tell from old-format log lines alone (the `" heuristic"` marker didn't exist until commit `e91d05a`, 2026-10-26 22:59:41 -0400). All four opcodes reappeared, sourced from a capture file timestamped 18:38, well before the marker existed. Fixed by hard-excluding any packet older than the marker commit's epoch (`HEURISTIC_MARKER_EPOCH` in the script) from the whole pool, not just new-opcode discovery - dropped FACTS-count inflation on ~20 other entries too, not only these four.

* `scripts/correlate.py`'s first two findings (`0x03e9`, `0x02b3`) were written into `opcodes.toml` AFTER the trailing `FACTS (...)` sentence instead of before it, so the very next `scripts/registry-facts.py` run - which deletes everything from the first `"FACTS ("` to the end of the string and replaces it - silently wiped both notes with no warning. Re-added before the FACTS marker, phrased with relative language ("most", "every one") instead of hardcoded counts so future refreshes can't make them read as stale. The script's own docstring now says this in capitals.

* `0x02fe` was first read as movement; it is the mob-movement report (`0x00db` is our movement).
* `0x0456` "sequential ids" were x coordinates; it is NPC movement.
* `0x03d5` "respawn" is a control assignment (26/26 are for mobs already announced).
* The `neon-stream` shuffle table in `outgoing.rs` was invented; replaced by the real table and the verified shuffle.
* The first attach-based capture's wire-length pairing was approximate; the exact length lives at `+0x428`.

Biggest remaining unexplained traffic as of 2026-10-26 overnight (92 incoming opcodes are still bare `observed`, mostly low-count):
`0x02af` (a 164-byte attack/skill variant that pairs a player and a mob and names the mob's template, but is not explained by the
`damage` model that works for `0x02a8`/`a9`/`aa`), the `PlayerSpawn`/`AvatarModified` ~280-byte avatar-look blob (skin, face, hair,
stats - needs WZ-style reference ranges to decode, not attempted), the movement fragment's per-fragment "stance" byte and the
climbing/rope fragment layout, and everything that needs an action nobody has triggered yet (trade, most skills, buffs, parties
with a second live player, the lie detector). None of these block "the mapping" in the sense the registry cares about: every
opcode seen has at least a FACTS-level structural description (size, periodicity, constant bytes, id references); what is
missing is a confirmed MEANING for the ones that still say "Meaning not established".

## Next

**2026-10-27: see `docs/decode-plan.md` for the full systematic plan** - every still-unnamed opcode, a defined
escalation ladder per opcode (passive tools -> static exe analysis -> family correlation -> live trigger), and the
Tier A/B replay-goal definition (byte-replay already works; building Rust encoders for the named opcodes is the
actual remaining target). `docs/decode-plan-tables.md` is the auto-generated status table
(`python3 scripts/decode-status.py > docs/decode-plan-tables.md`) - regenerate it instead of hand-tracking opcode
status here. The list below is kept for its specific narrative history; the live status lives in that plan now.

1. The wire opcode numbering (see Stage B above) - the one open item with real consequences (it blocks going online).
2. More live actions on jib01 with marks for opcodes that need a trigger nobody has done yet (trade needs a second account; most
   skills need levels this beginner character does not have).
3. ~~The avatar-look blob in `PlayerSpawn`/`AvatarModified`~~ - face and hair CONFIRMED tonight (2026-10-27, passive
   only, no live actions). `AvatarModified`'s pre-equip header is a fixed 20 bytes: u16@13 is the face id (100% in
   the real MapleStory face-id range 20000-22101, 62 samples) and u16@22 is the hair id (100%, range 30000-32035).
   Cross-validated against `PlayerSpawn`'s own (variable-offset) header for the same 60 char_ids seen in both
   opcodes: face matched 60/60, hair matched 60/60 - an independent-source confirmation, not a guess. Added as real
   `field` entries on `AvatarModified`; `PlayerSpawn`'s copy is documented in its summary but not yet a `field`
   entry since its offset shifts with name length (needs a `parse_look`-style helper like `decode::parse_equips`).
   Gender and skin CONFIRMED the same night, same method: a naive "same back-offset from face" guess only matched
   89%/61%, so a brute-force per-offset scan against the two opcodes' headers found the real offsets - u8@7
   (AvatarModified) / u8@22 (PlayerSpawn) is gender (0=male/1=female), u8@8 / u8@388 is skin color (0-5); both
   50/50 exact matches for every shared char_id. Added as fields on `AvatarModified` too.
   Earlier same-night exploration along the way (kept for the trail): found the player's own name repeats verbatim
   in `PlayerSpawn`, tagged `u16=2`, right after a fixed 285-byte block whose other bytes (including a 16-value
   per-player field at one offset) are still unexplained - that tag is NOT the face id, corrected in opcodes.toml.
4. ~~The movement fragment's stance/climbing byte layout~~ - SOLVED tonight (2026-10-26, active climb on jib01): climbing uses the same `MoveSelf`/`0x00db` opcode and x@16/y@18 header as walking; the per-fragment stance byte is 15 exactly when foothold is -1 (no platform underneath - the ladder case), a clean separation across 152 real fragments, never seen on any of the 141 fragments with a real foothold. Also fixed an off-by-one in the opcode's own records table (`start` was an unverified 26, should be 25).
5. Once the wire numbering is solved offline against recorded connections: NP12-T first, with `neon-cmd`'s `SafetyGate` and
   `ReconnectPolicy` enforced, per the approval in `docs/authorization.md`.
6. Systematic sweep (2026-10-27, new): run `scripts/correlate.py` over the full pool each session and work its
   ranked output top-down, instead of picking the next opcode to study by eye from the Opcodes/Todo tables. First
   run already surfaced `0x03e9` and `0x02b3` (see the tools table above) - re-run periodically as the pool grows,
   since a correlation that was too thin to trust (few packets) can become solid with more data, the same way the
   `0x0441` kill-correlation got stronger from 116 to 32,677 kills across this session.
