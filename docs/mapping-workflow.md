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
| `neon-calib/tools/poscorr.py` | Does a field hold the position of the entity the packet refers to? | attacker position pairs in `0x02a8` (77% within 30 px) |
| `neon-calib/tools/actlab.py` | Which opcodes does one isolated action produce (baseline + repeats)? | party create/leave |
| Map geometry (`neon-calib/maps/<id>.json`) | Do decoded positions lie on real platforms / at real NPC positions? | spawns 383/384, NPC moves vs the NPC list |
| **Marks** + `neon-calib/tools/marklab.py MARKS LOG...` | Which opcodes follow a hand-made action? Type a label into the page's Mark box (stored in `run/marks.jsonl`, `POST /mark?label=`) when you do something in the game; `marklab.py run/marks.jsonl run/sessions/X.log` lists, per mark, the opcodes whose count in the next 15 s is far above their background rate. Repeat the action: an opcode that follows every repeat is the candidate (then confirm with the evidence rules). | needs logs with real capture times |
| Aligned outgoing capture | Which sent packet belongs to which key press? | attack `0x00e1` at each `ctrl`; movement `0x00db` |

**Marks pitfall (seen 2026-10-26):** never run an action loop whose period is a round number (60 s) without checking the periodicity first: `periodic.py` showed `0x004f` fires every 60 s, which phase-locked with a 60 s test loop and looked like "3/3 repeats" of opening the stats window. Vary the gaps (random 5-20 s) and run `periodic.py` on candidates before trusting them.

**Negative result (2026-10-26, jib01, Amherst):** opening and closing the inventory, equipment, stats, skills and quest windows (`i e s k q`), 4 repeats each with random 5-17 s gaps, produced no opcode that repeated in 3 of 4 repeats in either direction. These windows are client-only; they are not on the wire. Remaining candidates need a server round trip: shop/NPC dialog, trade, party invite, skill use, buff, item use, chat channels.

**Negative result 2:** after walking jib01 next to the Amherst mailbox, 4 clicks on it (random 6-15 s gaps) opened no window and produced no repeating opcode (candidates appeared in at most 1 of 4 windows and were ordinary town traffic). Walking by held arrow key (`xdotool keydown Left`, 3.2 s) works on the test account.

## Evidence rules (what gets a name)

1. A **name** needs an independent check, not a plausible story: a known value from another source (memory reader, map file, EXP counter),
   an isolated action that reproduces it, or a pairing/ id match that chance does not explain. Record the numbers in the entry's `summary`.
2. `verified` additionally needs a **real packet as a test fixture** that decodes as that variant. The registry test fails otherwise.
3. Guesses stay as `structure` with a summary that says "unverified"; wrong guesses are corrected in place (`0x02fe` was first read as
   movement, `0x0456` as ids; both were wrong and the entries say so).
4. Fields carry **bounds** (`min`/`max`/`charset`) so decoding noise (the incoming cipher key is only estimated, see the protocol notes) is rejected.

## Status (2026-10-26, end of the first mapping day)

Registry: **140 incoming opcodes** (19 with a name; 20 verified entries, 7 decoded, 20 structure, 104 observed only) and **29 outgoing opcodes** (7 with a name, the rest structure or observed).

Named incoming: Chat, DropRemove, DropSpawnItem, DropSpawnMeso, ExpMessage, InventoryChange, LoginInventory, MapEnter, MobControl, MobHit, MobKill, MobMove, MobSpawn, NpcMove, PartyCreated, PartyJoined, PartyLeft, PetMove, PetSpawn, PickupMessage, PlayerAttack, PlayerMove, PlayerSpawn, QuestMessage, QuestRecordMessage, StatUpdate.
Named outgoing: Attack, AttackSkill, KeepAlive, MoveMob, MoveSelf, PartyCreateRequest, PartyLeaveRequest, PickupDrop, UiAction.

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
| `MoveMob` and `MobControl` | outgoing mob moves are only for mobs whose `0x03d5` carried our id (5/5); all 26 `0x03d5` are for already-announced mobs |

### Corrections made along the way (kept visible on purpose)

* `0x003d`, `0x002c`, `0x006e`, `0x023f` ("1 s ticks", counters with constant `fe`/`fc` tails) never existed: they were frames the old per-block heuristic decrypted with the wrong constant. After exact decryption (docs/network-protocol.md, 2026-10-26 18:40) none of them occurs in 1,600 packets (they were 900 in 30 min). The registry entries are removed. Treat any entry derived from logs written before commit `ab800c1` as unconfirmed until it reappears in an exact-decoded log.

* `0x02fe` was first read as movement; it is the mob-movement report (`0x00db` is our movement).
* `0x0456` "sequential ids" were x coordinates; it is NPC movement.
* `0x03d5` "respawn" is a control assignment (26/26 are for mobs already announced).
* The `neon-stream` shuffle table in `outgoing.rs` was invented; replaced by the real table and the verified shuffle.
* The first attach-based capture's wire-length pairing was approximate; the exact length lives at `+0x428`.

Biggest remaining unexplained traffic (open the **Map next** panel): player and mob spawn headers, `0x003d` (a sequence counter, ~30 streams), `0x02a9`/`0x02aa`/`0x02af` variants, the outgoing `0x00e7`, `0x013f`, `0x0407`, `0x0265`/`0x0266`, telemetry (`0x041f`-`0x0425`), and everything that needs an action nobody has done yet (shops, NPC dialogs, trade, skills, buffs, party invites, the lie detector).

## Next

1. More aligned outgoing captures per action (skill, item use, NPC talk, shop, chat); keep attaches under ~20 s.
2. Fragment field meanings (stance values, the byte after the command) from data with known stances.
3. Packet-only world model: own position from `MoveSelf` is in; map id and HP/MP come from the memory reader today.
4. Stage B questions (headless without the client) stay gated on written Nexon approval (`docs/headless-plan.md`).
