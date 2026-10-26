# Headless Neon: architecture plan

Goal: a mode where Neon runs with no game UI in front of anyone, every packet is understood, and our own UI is the
view into the game. Two stages: **(A) official client stays the connection** (MVP), **(B) no official client**
(later, gated).

## Where we are (2026-10-26)

| Piece | State |
|---|---|
| Incoming decode | ~15 of 140 seen opcodes verified (mobs, players, drops, own stats, party, pickups, chat); passive via tcpdump, `neon-stream/src/net.rs` |
| Outgoing read | Opcode + fields readable from the client's packet object before encryption (send-packet fn `0x141660360`, build-specific); 25 opcodes seen, almost none labelled; needs a debugger attach |
| Outgoing write | Header, IV schedule and **payload cipher solved** (2026-10-26: AES-256-OFB, `neon-stream/src/send.rs`); wire opcode numbers are scrambled per connection (open); nothing has ever been injected |
| Input | XTEST keys/clicks per Xvfb display; works, live-tested |
| State | Memory roots for position/HP/MP/level/map; footholds; navigation; brain (`brain.rs`) is pure state-in/actions-out |
| Live-account caveat | gdb attaches of 60-150 s dropped both test clients; see `docs/network-protocol.md` |

## Gates (decide before the step they guard)

1. **Authorization scope.** MSCW-BOT-AUTH-2026-1023-KS covers automation *through the normal game client*. Stage A fits.
   Stage B (own network client, own login, own packet writer) and any packet injection are outside what is written;
   get Kevin Song's explicit yes (in writing, added to `docs/authorization.md`) before step B1.
2. **Debugger use on live accounts.** Only short attaches, on a test account, supervised. Prefer passive capture.
3. **Credentials.** Stage B needs the login flow without the launcher; do not store tokens in the repo or logs
   (some packets carry the launcher login token; captures must never be committed).

## Target architecture

```
        game server
            |
   [ transport ]   A: official client + tcpdump (in) / client packet object (out)     B: our TCP client + crypto
            |
   [ protocol ]    opcode table (one source of truth) -> typed events in, typed commands out
            |
   [ world model ] map, self, mobs, players, drops, inventory, party, quests, chat (event-sourced, replayable)
            |                         \
   [ brain / plans / safety ]          [ view API ]  WebSocket + HTTP: snapshots + event stream
            |                                  |
   [ actuator ]    A: XTEST keys/clicks      B: typed commands -> packets       [ Neon UI ] our own map/inventory/chat view
```

Rules that keep both stages on one codebase: the brain only sees the world model and emits **commands**
(`Move`, `Attack`, `UseItem`, `Pickup`, ...); only the actuator knows if a command becomes a key press (A) or a packet (B).
The UI only reads the view API (it never touches the game or the brain directly).

## Stage A: official client as the connection (MVP)

**A1. Opcode registry (single source of truth).** `neon-protocol/` crate with `opcodes.toml` (id, direction, name,
status: unknown/structure/verified, field layout, evidence link) generating Rust decoders and `docs/opcodes.md`.
Replaces `scripts/opcode-doc.py`'s hand tables. Every decoder ships with real captured packets as tests.

**A2. Safe capture harness.** Timestamped passive capture of incoming (existing `NEON_DUMP_OPS`) plus an action
runner (xdotool) that performs one action at a time and logs times; auto-diffs against an idle baseline.
Outgoing read behind an explicit `--outgoing-read` flag with a hard time cap (default 10 s) and test-account guard.

**A3. Map the opcodes by action, in this order** (each: isolate one action on a quiet map, record in+out, label only
with evidence, add a unit test from the capture):
1. session: login, world/channel/character select, enter map, change map, change channel
2. self: stats (all bits), HP/MP, EXP, level-up, AP/SP, buffs, death/revive, job
3. movement and combat: own move/attack/skill, mob control, damage, kill, loot, pickup
4. inventory/equipment/shops/NPC/trade/storage
5. social: chat (all channels), whisper, party, guild, buddy, messenger
6. systems: quests, skills window, key bindings, pets, minimap, events
7. safety signals: lie detector (needs a real check), GM presence, server notices/maintenance
Exit criterion: every opcode seen in a 24 h capture is either labelled or listed as "ignored, with reason".

**A4. World model.** One event-sourced `neon-world` crate: apply typed events, expose snapshots, support replay from a
capture file (so everything is testable offline). Retire direct memory reads as a *source* once the network model
covers it; keep them as a cross-check (the existing position/HP roots are the oracle for validating decoders).

**A5. Command layer.** Brain emits typed commands; actuator A maps them to keys/clicks. Add verification: every
command has an expected world-model effect and a timeout (this replaces today's open-loop key sending).

**A6. Headless display.** Official client on Xvfb per account (already works, see `docs/instances.md`); add supervisor
for crashes/disconnects ("Unable to connect" recovery without hammering the login server: backoff, max retries,
alert a human).

**A7. View API and Neon UI.** Server exposes: `GET /state` (snapshot), `WS /events` (typed stream), `POST /command`
(through the same safety layer as the brain). New UI is a thin client: live map (footholds from the client's map data,
mobs/players/drops), inventory, stats, chat, party, packet inspector (opcode, decoded fields, unknown bytes), recorder
and replayer. The existing `neon-web` panel becomes this UI; remove anything that needs the game window.

**A8. Safety layer (shared by A and B).** Rate limits, human-like timing (authorization asks ±25%), session caps,
GM/stranger pause, lie-detector escalation to a person, maintenance windows, peak-hour avoidance. All commands pass it.

**MVP definition:** official client running unseen on Xvfb; no screenshots or window needed for operation; UI shows
a live, correct view of the game from decoded packets; bot drives via keys; replay of a capture reproduces the
same UI; 24 h unattended run with no unlabelled-opcode surprises on the logs.

## Stage B: fully headless (no official client). Gated

**B0. Gate:** written Nexon approval (Gate 1). Also decide how to handle the client's security module: do not
build anything that defeats or hides from it without that approval; if approval is not given, stage B stops here.

**B1. Handshake and login transport.** Reimplement connect, hello (IV exchange; send/recv IV schedule is already
solved), version/patch checks, account login, world/channel/character select. Needs a clean capture from connection
start (`neon-calib/captures/connection_start.pcap` is the seed; keep captures local).

**B2. Outgoing cipher.** The unsolved piece: the payload cipher runs in the client's own code. Options, in order of
preference: read the algorithm from the client's (plain, not virtualised) send path statically and re-implement it;
verify it against ciphertext/plaintext pairs from A3 (we can pair them: plaintext from the packet object, ciphertext
from the wire). Only then write packets. No injection into a real session before a replay test passes offline.

**B3. Opcode coverage for commands.** Every command the brain uses needs its outgoing layout verified from A3
captures (opcode + fields), plus the sequencing rules the server expects (counters, tick fields, move fragments).

**B4. Parity run.** Run A and B side by side on two test accounts doing the same plan; compare world models and
server acceptance (no desyncs, no kicks).

**B5. Cutover.** B becomes the default for accounts with approval; A remains as fallback and as the oracle.

## Milestones and order

| # | Deliverable | Depends on |
|---|---|---|
| M1 | `neon-protocol` registry + generated docs, all verified opcodes migrated | - |
| M2 | Capture harness with action runner and baseline diff | M1 |
| M3 | A3.1-A3.3 mapped (session, self, movement/combat) | M2 |
| M4 | `neon-world` with replay; UI reads it | M1, M3 |
| M5 | Command layer + verification loop; brain on commands | M4 |
| M6 | A3.4-A3.7 mapped | M2 |
| M7 | View API + Neon UI; **MVP exit test** | M4, M5 |
| M8 | Gate 1 decision; if yes, B1-B2 offline (replay tests only) | M6, Gate 1 |
| M9 | B3-B5 | M8 |

## Risks

* Build-specific addresses (send function) break on every client patch: keep the locator scripted and re-runnable.
* The lie detector opcode only appears during a real check; plan for passive monitoring with alerts, never a guessed solver.
* Live debugger sessions can cost an account session: keep them short, on test accounts, off the main account.
* "Full opcode map" is open-ended: define done as the A3 exit criterion above, not "every opcode the server could send".

## Status log

* 2026-10-26 M1 `neon-protocol` (registry, 165 opcodes, constraints, real-packet tests), M2 `neon-calib/tools/actlab.py`
  (baseline + repeats; validated on party create/leave), M4 `neon-world` (events, state, log replay, JSON snapshots),
  first slice of M7: `neon-world/src/bin/neon-view.rs` serves `/` (page), `/state`, `/events` (SSE) on 0.0.0.0:8765 from a
  packet log or a live `--follow` capture. Run:
  `cd neon-world && cargo run --release --bin neon-view -- --log <stamped log> --follow --speed 0`
  (log = `NEON_DUMP_OPS=all neon-stream --net ... 2> >(python3 stamp.py log)`). No authentication: LAN only.
* Found and fixed on the way: `neon-stream` did not build (invented shuffle table in `outgoing.rs`); IV shuffle now
  matches the algorithm verified on a real connection start.
* 2026-10-26 M5 started: `neon-cmd` (typed `Command`s, `SafetyGate` with the authorization's limits: +-25% timing jitter,
  rate cap, idle breaks every 45-90 min of 30-180 s, 6 h session cap, named pauses such as `gm` / `lie_detector`;
  `XtestActuator` building xdotool argv per command; `Executor` that checks each command's expected effect on the world
  model and reports Confirmed / TimedOut / Failed; `StaffRules` pausing the gate when a staff-named player is on the map).
  Offline-tested only; not wired to the brain yet (the brain still presses keys directly).
* 2026-10-26 (evening) milestone state. **M1** registry (variants, records, bounds, evidence rule per variant), **M2** harness + tools
  (`actlab`, `idscan`, `recfind`, `poscorr`, `autocorr`, `pairscan`, guarded `capture-out.sh`), **M4** world model with replay,
  **M5** command layer (offline; includes the Sunday maintenance pause), **M7** Neon View as its own crate: live + replay (play/pause/
  speed/seek), several logs merged by time (incoming + outgoing), opcode explorer with byte map and a ranked "map next" list, packet
  inspector, chat, activity, party, players with level and equipment, mobs with HP, NPCs, drops with items/mesos, own position from
  our own outgoing movement. Registry: 140 incoming opcodes (20 named) and 29 outgoing (7 named).
  Still ahead: M3/M6 (the long tail of opcodes needs actions nobody has done yet: shops, NPC dialogs, trade, skills, buffs, party
  invites, the lie detector), M8/M9 (Stage B, gated on written Nexon approval, plus the unsolved payload cipher in both directions).
* 2026-10-26 (19:30) **Both ciphers solved and implemented, passively.** Incoming: `plain = cipher - iv[0]` per packet, IV shuffled per packet, every header verifies the IV chain
  (`neon-stream/src/recv.rs`); the old heuristic was wrong for ~6% of frames and invented four opcodes (removed). Outgoing: AES-256-OFB with the 4-byte IV repeated four times and a
  static key (`send.rs`, `aes.rs`); the send IV is learned from the headers too. Details and evidence: `docs/network-protocol.md`. The client->server wire opcodes are scrambled per connection
  (`WireMap`, `wiremap.py`); how a connection's numbering is chosen is the next open question for Stage B (B2 is therefore no longer "the payload cipher"; it is the opcode numbering).
  Nothing was sent; Stage B stays gated on written Nexon approval.

