# Navigation

The bot plans routes over the real map geometry instead of guessing ("mob is above me, so jump").

## Where the geometry comes from

The client ships every map's footholds, ladders/ropes and portals in its own data files:

```
.../Nexon/Library/maplestorycw/appdata/Data/Map/Map/Map0/Map0_000.wz   (maps 0xxxxxxxx; id 10001010 = 010001010.img)
```

`data/tools/wzmap.py` parses the WZ archive (unencrypted `PKG1`, version-hashed offsets, no
dependencies) and writes `data/maps/<mapid>.json` (334 maps, ~6 MB, 3 s). Re-run it after a client patch:

```bash
python3 -I data/tools/wzmap.py "<path to Map0_000.wz>" data/maps
```

Verified against live play: platform heights in Henesys Hunting Ground I (215, -85, -325, -565) and the
east portal at x=1625 match the measured values. This replaces the in-memory foothold scan (which is
still in `neon-capture` as a fallback) and also gives ladders/ropes, which memory never did.

## The graph (`neon-loader/src/nav.rs`)

Nodes are walkable segments. Edges are the moves that exist, each costed from where the previous move landed:

| Move | When |
|---|---|
| Walk | segment ends touch |
| Fall | walk off a free end onto the nearest segment below |
| Drop through | hold Down + jump, unless the platform forbids it (`no_drop`) |
| Climb | any ladder/rope between the segments it touches; a rope (or a ladder end more than 30 px above the floor) is grabbed by jumping while holding Up |
| Jump up | standing jump onto a platform up to 72 px higher (measured: the Beginner jump rises 78 px), or a running jump across a gap <= 60 px |

`Nav::step` returns one command per tick (`Walk`, `Jump`, `DropThrough`, `Climb`, `Arrived`, `NoRoute`) and replans
from the current position every call, so a missed jump or knock-back just produces a new route. The brain
picks the mob with the cheapest route (not the nearest by distance), blacklists mobs with no route, and gives
up on a target after 10 failed maneuvers from one platform.

## Safety rules that came out of live runs

* Idle breaks wait until the character is grounded, healthy, off ropes and no mob is within 350 px.
* `--mobs 2,3,4,5,6,7` limits fighting to snails/shrooms/stumps/slimes (pigs and mushrooms hurt a level-7 Beginner).
* While recovering below the flee threshold the bot steps away from adjacent mobs instead of standing in them.
* Potion keys are `F1`/`F2`/`F3` (xdotool key names are case-sensitive).

## Live status / UI

`neon-loader` writes `neon-status.json` twice a second (state, stats, events, level progress, map outline).
`cd neon-web && npm run dev` serves it at `/api/live`; the page is a single dashboard (vitals, session stats,
level-progress chart, map with platforms/ropes/mobs, event log).

## Combat range (from meowdb.com/msclassic/formulas/targeting)

A hit needs the attack box and the mob's hitbox to overlap on both axes. Barehanded punch (the `ctrl` attack with no weapon):
64 px horizontal reach from the character, 33 px above the feet. The bot walks up to a **38 px stand-off**
(`stand_off_x`), swings from **52 px** (`attack_range_x`, 64 minus a margin for position error and mob width) and only when
the mob is within **28 px** vertically (`attack_range_y`). Changing weapons changes these (a 1H sword is 85 x 51).

## Latency and accuracy (measured live, 2026-10-24)

* Stream runs at 50 fps (`--interval 20`); a frame is ~0.1 ms old when the bot decides (`latency_ms` in the UI).
* Keys go out through an in-process XTEST connection (`neon-loader/src/xkeys.rs`), not a spawned `xdotool` per key;
  taps are scheduled key-up events, so a swing never blocks the decision loop.
* The client's own position memory only changes every ~60-100 ms; that is the floor.
* The map-id debounce is time-based (600 ms): five frames at 50 fps is only 100 ms, shorter than the loading garbage.
* A stale position may stand in for a failed read for at most 150 ms; physically impossible jumps are rejected by the brain.
* Mobs: the server only reports where each move *starts*, so each mob's speed is estimated from successive updates and its
  position extrapolated up to 1 s.
* Own outgoing packets are not used: the client->server direction is a per-packet keyed stream cipher (the header seed differs every
  packet, even identical keep-alives differ), so it cannot be read from the wire without the client's key.

## Debugging

`neon-loader --trace-file decisions.log` writes one line per tick: timestamp, position, grounded flag, target, the brain's
decision and the actions emitted. `--only-map 10001030` keeps the bot in Forest West of Henesys.

## Shadow simulation (sim.rs)

A model character runs beside the real one, driven by the same inputs (held direction, jumps, drop-downs) on the
map's footholds: walking with acceleration and a slide, jumps, gravity, landing, walls. Every bot run feeds it; the
panel's map draws it as a dashed ghost joined to the real position, with the running error under the map.

* `neon-loader --sim-log FILE` logs one line per frame (`ts dt dir jump down climb ax ay agr sx sy sgr age err`).
* `data/tools/simcal.py OUT.trace` records a fixed set of moves in a town (walks, standing/running jumps,
  direction+jump together, run-ups of 0.08/0.15/0.3 s, release in the air, air control, key taps) as a decision trace.
* `neon-loader --sim-replay TRACE MAP --sim-log OUT [--sim-resync-ms 1000]` replays any decision trace through it.
* `neon-loader --sim-fit MAP TRACE...` fits all constants by coordinate descent on the average error (1 s windows,
  re-synced on the ground). `data/tools/simfit.py LOG` summarises a log.

Findings from two recordings on Lith Harbor's flat floor (x 1112-2218, y 647), 2026-10-25:

* The memory position updates in bursts: two new values, then ~60 ms of repeats. The grounded flag flips before y
  catches up. Only changed values are scored.
* Walking ~146 px/s, reached almost at once; ~10-15 px slide after release.
* The sideways speed of a jump is set at take-off: with a direction held (even pressed in the same instant as the
  jump, from a standstill) it is ~134-150 px/s, otherwise the ground speed (a standing jump goes straight up).
  Releasing the direction or pressing one in the air hardly changes it.
* Jump: up ~550 px/s, gravity ~2000 px/s^2 (rise ~76 px, peak ~0.28 s).
* Fitted model error: 5.8 px on the ground, 10 px in the air (the read's own staleness is ~9 px at walking speed).
  The navigator still uses 170 px/s for the air (nav.rs), which made the measured climb repeatable; the fitted
  134-150 px/s has not been checked on real platform jumps yet (the hunting-map traces are dominated by read
  glitches and ropes).
