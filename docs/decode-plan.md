# Full protocol decode & replay plan (2026-10-27)

Why this exists: the overnight mapping work was picking the next opcode to study by eye, cycle after cycle, with
`scripts/correlate.py` running clean (0 new hits) for the last ~10 cycles straight - the easy wins from pure
passive log-watching are exhausted. This is the systematic plan that replaces "pick one and see" with a defined
escalation ladder per opcode, a live status table that can never go stale, and - the piece that was missing - a
path to actually read the game client's own code for the opcodes passive data can't crack.

## The actual goal: two tiers of "replay"

"The traffic can be replayed" has two very different bars. Be explicit about which one a task is solving:

- **Tier A - mechanical replay (already works today).** `neon-stream --net-replay` already reconstructs the exact
  wire bytes for a captured session once the cipher state is known, because it preserves raw bytes regardless of
  whether their *meaning* is understood. Every opcode, named or not, already replays at this tier. This tier is
  NOT what's blocking anything further - don't re-derive it.
- **Tier B - semantic replay / synthesis (the actual target of this plan).** Constructing *new*, valid traffic for
  an opcode from field values - e.g. the bot sending a legitimate `MoveSelf`, or a test harness emitting a
  synthetic `MobSpawn` to exercise `neon-world` without a live capture. This needs, per opcode:
  1. A confirmed field layout (decode) - what most of this project's work so far has produced.
  2. A Rust **encoder** that is the exact inverse of the decoder (`encode(decode(bytes)) == bytes` for every real
     sample, as a test). **This does not exist yet for a single opcode** - `neon-protocol` only decodes
     (see `docs/decode-plan-tables.md`'s "Tier B candidates" list: 37 named-with-fields opcodes, 0 encoders).
  3. For outgoing opcodes specifically, this is what Stage B (real bot play) actually needs - a constructed
     `MoveSelf`/attack/etc. byte-for-byte correct, not just a replayed capture.

Tier B encoders are new work this plan should start producing, not just more decoding - see "Building encoders" below.

## Per-opcode escalation ladder

Climb these in order; record progress (even negative results) in the opcode's `summary` so later passes don't
repeat a dead end. Stop as soon as `docs/mapping-workflow.md`'s evidence rules are satisfied for a name.

1. **Passive structural tools** (cheap, safe, already built, no live risk): `recfind.py`, `idscan.py`,
   `poscorr.py`, `lifecycle.py`, `registry-facts.py`'s auto field detection, and **`scripts/correlate.py`**
   (kill/spawn/leave timing sweep - run this FIRST, every session, before picking anything by eye).
2. **Static binary analysis of the client exe** (see "How to read the exe" below) - the step this plan adds.
   For a high-traffic opcode where step 1 has already produced its strongest possible passive signal (flagged
   "needs binary analysis" in the status table), the client's own handler code is the only thing left that can
   disambiguate *what the bytes mean*, as opposed to *when the packet tends to occur*. This is the same
   no-debugger-attach, `/proc/PID/mem` + `objdump` method that solved both the send and receive ciphers
   (`docs/network-protocol.md`) - it is proven safe for this project, unlike attaching a debugger (which has
   twice dropped both test accounts' sessions, see `gdb-attach-drops-sessions` in memory).
3. **Cross-opcode / family correlation**: does this opcode's shape, timing, or id-field match an
   already-confirmed sibling (the `0x0279`/`0x02bb`/`0x02bd`/`0x0281` post-spawn family is the model case)?
4. **Live, isolated action trigger** (marks + `marklab.py`), only when a plausible, safe, authorized trigger
   exists (see `docs/authorization.md` and the Stage B gating). Lowest priority precisely because it is the only
   step with real-world risk (session cost, account safety) - exhaust 1-3 first.
5. **Independent confirmation**: a name needs at least two of the above to agree, or one very strong single
   signal (100% correlation over a large, varied sample, like `0x0441`'s kill timing or the face/hair/gender/skin
   cross-opcode match) - see the evidence rules in `docs/mapping-workflow.md`.
6. **Build the Tier B encoder** once decode is confirmed (see below) - this step is new; nothing has reached it
   yet for any opcode.

## How to read the exe (step 2, safely)

Already-proven method, used for both the send and receive cipher (`docs/network-protocol.md`), **zero debugger
attaches**:

```
# read live process memory without attaching (no ptrace, nothing the server can detect as a debugger):
dd if=/proc/$PID/mem bs=1 skip=$((0x<addr>)) count=<n> of=dump.bin
objdump -D -b binary -mi386:x86-64 --adjust-vma=0x<addr> dump.bin
```

Known, already-located entry points to start from (Classic World v1.0.1, build-specific - re-verify addresses
after any client update):

- **Receive dispatch**: `0x1416667a0`, reached via framer `0x14075c780` -> decrypt `0x14075ccb0` -> IV advance
  `0x140cec340` -> dispatch. This is the per-opcode jump/switch - tracing it finds each internal opcode's own
  handler function address. **This is the concrete next action for every "needs binary analysis" opcode**: find
  this packet's case in the dispatch, read its handler, and see what it does with each byte range (compares
  against a known UI constant, calls a formatter, writes to a known stat field, etc. - the handler's *behavior*
  pins down a field's meaning in a way no amount of timing correlation can).
- **Send path / outgoing opcode construction**: `0x141660360` (packet object constructor, opcode at `+8`),
  frame builder around `0x1407618xx` (encrypts `obj+8`) - see `outgoing-opcode-method` in memory for the
  send-packet calibration tooling already built on this (`neon-calib/tools/outcap.sh`).
- **Per-connection wire-opcode scramble**: confirmed to live in the protector's virtualized section (not
  statically readable past `0x141664300`) - this is the one known case where static reading has already hit a
  wall; don't re-attempt it without a new idea (see `stage-b-status` in memory). Everything else in this plan is
  in the plain, readable part of the image.

Workflow for one opcode:
1. From the dispatch table at `0x1416667a0`, find the case for this opcode's internal id (same table format as
   whatever found the existing addresses above - extend the same `objdump` + cross-reference method).
2. Dump and disassemble that handler.
3. Match disassembled offsets/comparisons against the byte ranges `scripts/correlate.py`/the explorer already
   flagged as "unexplained, varying" - a `cmp` against a small constant, a call into a known formatter, or a
   write to a struct field whose other uses are already known, is the behavioral evidence a name needs.
4. Write the finding into the opcode's `summary` with the exact addresses read, so it's reproducible and doesn't
   need re-deriving after the next client update.

## Building Tier B encoders

Once an opcode's fields are confirmed (named, `status = "verified"` or `"decoded"`, with a `field` array):

1. Add an `encode` function to `neon-protocol` - the literal inverse of `decode_with` (reads `Decoded` fields,
   writes bytes at the registered offsets, fills anything not covered by a field with the recorded constant
   bytes from the opcode's own `FACTS` text).
2. Round-trip test: for every real captured sample of that opcode, `encode(decode(sample)) == sample` (byte for
   byte). This is a strong, free correctness check - any field offset/size/type error shows up immediately as a
   round-trip mismatch, independent of whether the *meaning* assigned to the field happens to be right.
3. Start with the 37 Tier B candidates in `docs/decode-plan-tables.md` (named, have fields, zero encoders today).
   `MobSpawn`/`MobMove`/`PlayerMove`/`DropSpawnItem` are the best first targets - simple, fixed-ish layouts,
   high traffic for round-trip testing.

## Zero-count opcodes (68 incoming + 23 outgoing)

These have never been seen once in 14+ hours of organic multiplayer traffic across both test accounts. They are
not "unexplored" in the normal sense - passive tools have nothing to work with. Two real possibilities, not
mutually exclusive:
- Rare/conditional events (a specific zone, item, or game mode this session never touched).
- Dead/unused opcodes in this client build (old protocol cruft).
Don't spend a cycle on these without either (a) a specific known trigger nobody has tried yet, or (b) exhausting
every opcode with real traffic first. Re-run `scripts/decode-status.py` periodically - a 0-count opcode moving to
nonzero is itself a signal worth noting (it tells you what game event is rare enough to not have occurred yet).

## Status (generated, do not hand-edit)

See `docs/decode-plan-tables.md`. Regenerate before every planning review:

```
python3 scripts/decode-status.py > docs/decode-plan-tables.md
```

## Priority order for the next sessions

1. Run `scripts/correlate.py` every session, first, before anything else (standing instruction, already in
   `mapping-workflow.md`).
2. Work the "needs binary analysis" opcodes in traffic order via the exe-reading method above: `0x0441` (106k
   packets, already has the strongest possible passive signal - kill-credit disambiguation is waiting on this),
   `0x02af` (29k), `0x022e` (3.9k), `0x02b9` (3.5k), `0x0279`/`0x02bb` (already named-adjacent family members,
   confirming the remaining unnamed ones strengthens the whole family).
3. In parallel, start Tier B encoders for the simplest high-traffic named opcodes (`MobMove`, `PlayerMove`,
   `MobSpawn`) - this doesn't block on anything above and directly serves the stated end goal.
4. Only after 1-3 are exhausted for a given opcode, consider a live trigger - and only within the existing
   authorization and safety guardrails (`docs/authorization.md`, the 30%-HP stop-fighting guard, no debugger
   attach).
