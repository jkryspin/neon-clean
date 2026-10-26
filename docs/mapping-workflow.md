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

* **Incoming, passive (safe):** `scripts/view.sh INSTANCE` runs `neon-stream --net` through `tcpdump` into `run/view.INSTANCE.log`.
  Nothing touches the game process.
* **Outgoing, short attach (opt-in):** the opcode and plaintext are read from the client's packet object just before encryption,
  not by breaking the cipher. `neon-calib/tools/outraw.py` (via `sudo gdb -p PID`, about 12 s) logs the raw object and the wire
  length of each send; `neon-calib/tools/outbuild.py raw.jsonl out.nsr` turns it into exact plaintext (length at `+0x428`,
  validated 41/41 against the sends). **Keep the whole attach under ~20 s on a test account**: sessions of 60-150 s dropped both test clients on 2026-10-26, and a 34 s attach
  (12 s capture + slow attach/detach) dropped jib01 a few minutes later even though the memory reader still saw it alive right after; a 24 s one was fine
  (`docs/network-protocol.md`). Expect to re-login after a drop (`scripts/instance-launch.sh`, one attempt, then back off). Addresses are specific to Classic World v1.0.1.

## Finding structure (tools, in the order they pay off)

| Tool | Question it answers | Example result |
|---|---|---|
| Explorer (in the page, `/explore`, `/todo`) | Which bytes vary, which are constant, which hold known ids, what is still unexplained? | ranked `PlayerMove` first, so movement became the next job |
| `neon-calib/tools/recfind.py` | Is it `header + count byte + n fixed records`? (the count byte is a checksum chance cannot satisfy) | rediscovered MobMove/PlayerMove, found PetMove and NpcMove |
| `neon-calib/tools/idscan.py` | Which fields hold ids of mobs / players / drops we already know? | attack broadcasts: player at `@2`, mob at `@63` |
| `neon-calib/tools/poscorr.py` | Does a field hold the position of the entity the packet refers to? | attacker position pairs in `0x02a8` (77% within 30 px) |
| `neon-calib/tools/actlab.py` | Which opcodes does one isolated action produce (baseline + repeats)? | party create/leave |
| Map geometry (`neon-calib/maps/<id>.json`) | Do decoded positions lie on real platforms / at real NPC positions? | spawns 383/384, NPC moves vs the NPC list |
| Aligned outgoing capture | Which sent packet belongs to which key press? | attack `0x00e1` at each `ctrl`; movement `0x00db` |

## Evidence rules (what gets a name)

1. A **name** needs an independent check, not a plausible story: a known value from another source (memory reader, map file, EXP counter),
   an isolated action that reproduces it, or a pairing/ id match that chance does not explain. Record the numbers in the entry's `summary`.
2. `verified` additionally needs a **real packet as a test fixture** that decodes as that variant. The registry test fails otherwise.
3. Guesses stay as `structure` with a summary that says "unverified"; wrong guesses are corrected in place (`0x02fe` was first read as
   movement, `0x0456` as ids; both were wrong and the entries say so).
4. Fields carry **bounds** (`min`/`max`/`charset`) so decoding noise (the incoming cipher key is only estimated, see the protocol notes) is rejected.

## Status (2026-10-26)

Registry entries by status: incoming 18 verified, 8 decoded, 20 structure, 104 observed; outgoing 11 structure, 18 observed.
Opcodes with a name: incoming `PlayerMove` (fragments), `MobMove` (fragments), `PetMove`, `NpcMove`, `PlayerSpawn`, `MobSpawn`,
`MobRespawn`, `MobKill`, `MobHit`, `PlayerAttack` (+2 variants, position pairs), `DropSpawn`, `DropRemove`, `StatUpdate`,
`ExpMessage`, `QuestRecordMessage`, `PickupMessage`, `QuestMessage`, `PartyCreated/Joined/Left`, `MapEnter`, `LoginInventory`,
`InventoryChange`, `Chat`, `PetSpawn`; outgoing `MoveSelf`, `Attack`.

Biggest remaining unexplained traffic (open the **Map next** panel): attack-packet header bytes, pet and spawn headers,
`0x003d` (sequence counter, 30 streams), `0x02af`, `0x03e7`, chat text, the outgoing heartbeat/telemetry family
(`0x0265`, `0x0266`, `0x0407`, `0x041f`-`0x0425`), and everything that needs an action nobody has done yet (shops, NPC dialogs, trade,
skills, buffs, party invites, the lie detector).

## Next

1. More aligned outgoing captures per action (skill, item use, NPC talk, shop, chat) with `actlab`-style action logs.
2. Fragment field meanings (stance values, the byte after the command) from data with known stances.
3. Packet-only world model: own position from `MoveSelf` is in; map id and HP/MP come from the memory reader today.
4. Stage B questions (headless without the client) stay gated on written Nexon approval (`docs/headless-plan.md`).
