# Headless architecture: what exists today

Plan: `docs/headless-plan.md`. This page is the "where are we" companion: what runs, how to run it, what is proven.

```
 game server
     |  (TCP, passive tcpdump)
 neon-stream --net  ---->  stamped packet log  (NEON_DUMP_OPS=all, run/view.NAME.log)
 neon-stream (memory) --->  latest state line   (position, HP/MP, level, map id; run/view.NAME.mem.json)
                                 |
                          neon-protocol   registry (opcodes.toml) + generic field decoder + constraints
                                 |
                          neon-world      events -> world state (map, players, mobs, drops, party, me, inventory)
                                 |                 + inspector (recent packets, opcode histogram)
                          neon-view       HTTP: /  /state  /events (SSE)  /packets  /opcodes  /map/<id>
                                 |
                          browser (our UI)         neon-cmd: commands + safety gate (not wired to the brain yet)
```

## Crates

| Crate | What it is | Tests |
|---|---|---|
| `neon-protocol` | `opcodes.toml` (165 opcodes; 13 with tested field layouts), strict validation (no names without evidence), `Registry::decode` with min/max/charset constraints | real captured packets decoded to independently recorded values; rejects a real garbage packet |
| `neon-world` | `Event`s from packets, `World::apply`, `apply_mem`, log replay, JSON snapshot, inspector, view server + page | real packets, real stat/party/pickup captures, real-socket HTTP test, path-traversal test |
| `neon-cmd` | typed `Command`s, `SafetyGate` (+-25% jitter, rate cap, breaks every 45-90 min, 6 h cap, named pauses), `XtestActuator`, `Executor` (confirm / time out), `StaffRules`, `ReconnectPolicy` | offline, scripted worlds |
| `neon-stream` | existing passive reader; fixed this session (it did not build: invented shuffle table) | 78 tests incl. real IV chain from a connection start |

`scripts/test-headless.sh` runs all four.

## Run it

```bash
scripts/view.sh jib01 8765          # live view of an instance (client must be running)
scripts/view.sh shurella2 8766
scripts/view.sh stop jib01
# -> http://<this machine>:8765/   (binds 0.0.0.0, no authentication: LAN only)
cd neon-world && cargo run --release --bin neon-view -- --log some.log --speed 1   # replay a capture
```

Page: map with the client's real platforms/ladders (`neon-calib/maps/<id>.json`, picked automatically from the map id read
from memory, or typed in the box), mobs by template id, players with names, our character, HP/MP/level/position, party,
packet inspector (opcode, registry name, status, decoded fields, bytes; "hide verified" to see only what is not understood yet).

## What is proven (2026-10-26)

* Incoming: ~15 opcodes decode with verified meaning; 140 opcodes seen; 0.09% of 3,318 real packets in the "rare" tail.
* The view works live on two accounts (Amherst town, Perion hunting map) from passive capture + memory reads only.
* Outgoing opcodes are read from the client's packet object (`docs/opcodes.md`, 25 seen), not written; nothing has ever been sent.
* Not done: any packet writing, the receive cipher (the client's receive path is mapped in `docs/network-protocol.md`),
  most opcode meanings, the brain on `neon-cmd`.

## Safety notes learned the hard way

* gdb attaches of 60-150 s dropped both test clients; never attach to the main account unattended (`docs/network-protocol.md`).
* A refused login is not retried in a loop: `ReconnectPolicy` (1 min, 5 min, 15 min, 1 h; max 6 per day, then a human).
* The view has no authentication.


## Update (later on 2026-10-26)

* `neon-view` is now its own crate (server, UI, inspector, **opcode explorer**, **replay transport**). `neon-world` keeps the model and the
  `.nsr` session record/replayer. The registry supports variants and repeated records, so movement, attacks and NPCs are data.
* The mapping method, tools and evidence rules are in `docs/mapping-workflow.md`.
* Outgoing opcodes can be captured with exact plaintext (short, opt-in attach) and replayed like incoming ones; our own position is now
  derived from the packets we send.

* `neon-cmd`'s `SafetyGate` now also holds the authorization's **Sunday 06:00-08:00 KST maintenance** pause (needs the wall-clock anchor
  `start_unix_ms`; tested at the window edges). The live loader (`neon-loader`) already has ±25% jitter, idle breaks, a 5 h cap and
  5% mistakes (`neon-core/src/anti_detect.rs`), but no maintenance window or peak-hour rule: use plan time windows for those until
  the loader is moved onto the command layer.

* **Session browser:** `scripts/replay.sh [port]` serves Neon View with a "session..." picker (archived captures in `run/sessions/`, the live logs
  linked in as `live-NAME.log`, any `.nsr`/`.jsonl` dropped there). `GET /sessions`, `POST /replay/load?name=...` (plain file names only; refused
  on a live `--follow` server). Seeking and playing work as for any replay.
