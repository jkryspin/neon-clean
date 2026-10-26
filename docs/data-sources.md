# Where the bot's data comes from

Everything the bot sees arrives as one JSON line per tick (`neon_state::GameState`) from
`neon-stream`. This page records what each field is read from, how far it has been
verified, and what is still missing. Last verified: 2026-10-24, client MapleStory Classic
World v1.0.1 under WineCharm, 64-bit, fixed image base `0x140000000`.

Roots and offsets live in `neon-calib/chains.json`. A "chain" is: read the 8-byte pointer
at `root` (an address inside the exe image), add a field offset, read the value.

## Re-verify after any restart or client patch

```bash
nice -n 19 ./neon-stream/target/release/neon-stream --chains neon-calib/chains.json --verify
```

It prints what every chain reads. Compare against the on-screen HP/MP/level/position.
If a root reads `UNREADABLE`, the client was patched and the roots must be re-found.

## Status of each field

| Field | How | Verified |
|---|---|---|
| Position (x, y) | chain `0x143bf05a0` +0xF8/+0xFC | Across a restart and several map changes. **Flickers** to garbage (e.g. `(50858304, 0)`); the reader needs two agreeing reads and holds the last good value 400 ms |
| Position, per frame (f32) | chain `0x143bd6788` +0x48 +0x120 +0x98 (f32 x, y) | The client's own position, a frame fresher than the i32 copy. Used **only when it agrees with the i32 read within 40 px**. Verified in Lith Harbor, through a map change and back, and after a re-login (`neon-calib/tools/possources.py`). Four other chains found the same day matched in one map, froze after leaving a shop and read (0, 0) in the next map; they outvoted the right one and the bot walked on a frozen position. They are listed as `rejected` in chains.json |
| HP, MP, level | chain `0x143bf8e78` +0x360/+0x364/+0x160 | Across a restart. MP checked against a live change. Level not yet seen across a level-up |
| Grounded | chain `0x143bf05a0` −0x3E4 | Jump and rope. 1 on a foothold; 0 in the air **and on a rope** (cannot tell them apart). Flickers; needs two agreeing 0/1 reads |
| Map ID | chain `0x143bf05a0` −0xC98 | Three maps (10001000 Henesys, 10001020 Hill West, 10001030 Forest West). Reads garbage while loading; the stream debounces it over 5 frames |
| Footholds, ladders/ropes, portals | **client data files** (`neon-calib/tools/wzmap.py` -> `neon-calib/maps/<id>.json`, see `docs/navigation.md`); the memory signature scan remains as a fallback | Heights and portals match live measurements. Exact, no scanning |
| Mobs (id + position), other players (name + position) | **network capture** (`--net`, see `docs/network-protocol.md`): passive `sudo tcpdump`, decoded in Rust | Verified live: mob types/positions and player names match the screen. No static root exists or was found |
| Entities without the network | hitbox-rect signature scan (`--mobs rects`) | Fallback only; not classified (mob vs player vs NPC) |
| Max HP, max MP | remembered per level in `neon-calib/seen-max.json` | Heuristic. Wrong if the stream first runs below max at a level it has never seen full |
| EXP | **network** (`0x007e` stat update, bit `0x10000`), verified over three kills | Not in memory as a plain/float value. `exp_needed` = 560 at level 7 (matches 78 -> 13.93%) |
| AP, max HP/MP | network stat update (same packet), unverified | Needs a level-up to capture; max HP/MP otherwise remembered per level |
| Drops (own kills) | network (`0x0471`/`0x0472`) | Verified: position, owner, source mob |
| Drops, other players' names | none | Not found. Names exist only as UI string caches |
| Rope state | none | No flag found; infer from grounded=0 plus x snapped to a rope column |

Things that looked like roots and were not (do not rediscover them):

- **Vtable-histogram method (works, but led only to render data).** Scanning all writable
  memory for qwords inside the exe's read-only data (`0x14334b000-0x143b6e000`) gives
  ~14k distinct vtable values with instance counts in 4 s. Testing every class against known
  positions found: (a) the foothold class `0x1435a7de0` (152 instances, `0x120` apart in
  blocks of 16, position at +0x18/+0x20, linked to each other, no container pointer found);
  (b) sprite-layer classes `0x14353b9f0`, `0x143476a50`, `0x14340f550` whose float32 `(x, y)`
  pairs at +0x158 / +0x218 / +0x398 *do follow moving entities* but are position copies in
  the render graph, not one object per entity; (c) a 34-instance class that holds only the
  local player's name (UI windows). No entity-logic pool was found.
- Entity feet: the hitbox rect bottom equals the entity's true y exactly (checked on the local
  player), centre-x is within ~8 px of the true x, and rect x is NOT stored as an int pair
  elsewhere (only 66 exact hits for 39 feet, none moving).

- Sprite/render-layer graph under global `0x143bf0170`: owns the hitbox rects, not game logic.
- Foothold "roots" `0x143bd7fa0`, `0x143bf0170` paths: pointed at the right objects only in the
  map they were traced in; after a map change they read plain numbers.
- Three single-capture "rope" flags (`0x107064`, `0x106b5c`, `0x10713c`): failed a second run.
- Player names found in memory (`shurelia2`, ...): UI text/name-tag buffers, no logic object.

## Mob IDs (from meowdb.com/msclassic)

`neon-calib/mob-ids.json`. Classic World mob ids are small decimals (Snail 2, Blue Snail 3, Red Snail 5,
Stump 6, Slime 7, Pig 8, Orange Mushroom 9 ...). In memory the client keeps an id -> resource-path table
(UTF-16 `Mob/0000009.img`); verified, but spawned mobs do not point at it, so it does not yet give a mob list.
Sprite layers reach resource paths in ~3 pointer hops (`Character/Hair/...`, `UI/...`) and the avatar layers are
players' parts, BUT the resolver is unreliable: the shared resource-cache tree links everything, so a deeper search
can return an unrelated `Mob/...` string (verified false positive for vtable 0x1435c4698). It cannot yet classify
entities; a mob-instance class has not been confirmed.

## Position and map data must agree

The brain pauses ("position source or map data is wrong") when the character stands (grounded, not on a ladder) for over
2 s where the loaded map file has no floor. Audit of 2026-10-25 against recorded runs: 98.9% of settled grounded reads in
Lith Harbor and 95.6% in Thicket Around the Beach I lie on a floor or ladder of the map file; the rest are single landing
frames and `(0, 0)` read glitches. The map files (from the client's own `Map.wz`) were right; the one apparent mismatch
(the Lith Harbor Department Store) was the frozen position chain above. Check any source with
`neon-calib/tools/possources.py <pid>`: it prints every position source, the map id, and whether each stands on a floor.

## Method that worked, and what to avoid

Worked: find a value you can read off the screen (position, HP, a jump), search for it in a
**small** region, confirm with a second independent capture, and trace the owning object back
to a global in the exe's `.data` (`0x143b6e000–0x143c13000`) with an exact pointer search.

Avoid: whole-heap snapshots held in memory. Several of them exhausted RAM on the 15 GB
host, triggered the kernel OOM killer (it killed a k3s pod and the game client) and crashed
the session. Scans must run `nice -n 19`, in fixed-size chunks, with a read budget.

## Live-testing rules (learned the hard way)

- A level-7 character was killed by pigs during a map-change test. Do not enter unfamiliar
  maps with test input; abort and retreat when HP < 60%.
- `pgrep -f MapleStory.exe` and `pkill -f <script>` match the shell running the command.
  Use a Windows-path check (`C:\...\MapleStory.exe`) and kill by PID.
- The client's command line contains a one-time Nexon launch token. Never log or paste it.

See also `docs/launching.md` for restarting the client.

## Input facts verified live (Beginner, Henesys area)

- Attack: `ctrl` (xdotool name). 9 swings killed a Blue Snail in the hunting ground, costing 4 HP.
- Jump: `alt`. Rope: stand under it, hold `Up`, tap `alt` (x snaps to the rope column).
  Portals: stand on it and tap `Up`. Walk speed ~150 px/s, rope climb ~110 px/s.
- Key release has ~0.15 s latency (~24 px overshoot). Use short corrective taps near a target.
- West portal in Henesys (x = -438) leads to *Henesys Hunting Ground I* (map 10001010), arrival
  x = 1625. That map is crowded; mobs are contested and orange mushrooms/snails chip 15-30 HP
  in a few seconds. Fight beside the portal and leave below ~105 HP.
- In towns, nametags such as "Snail" / "Black Pig" under a sprite are players' **pets**, and
  "Maple Administrator" is the standard information NPC, not a GM.
