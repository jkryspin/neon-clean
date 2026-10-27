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

## Step 1 RESULT (2026-10-27): public prior art works, and gives a validated alignment key

**Do this before any disassembly.** MapleStory's protocol is extensively documented by open-source server
emulators. Crucially, the [Kinoko](https://github.com/iw2d/kinoko) emulator's `OutHeader` table uses the
CLIENT's own internal handler naming (`CUserPool`, `CMobPool`, `CWvsContext`), so its opcode ORDER reflects the
real client enum, not a third-party invention.

Our opcode numbers are much larger than Kinoko's (ours reach `0x062e`, a far newer version), so the numbers do
not map. **But the order does.** Aligning Kinoko's table against our 13 already-named opcodes:

| Kinoko | name | ours | our name |
|---|---|---|---|
| 179 | UserEnterField | `0x022d` | PlayerSpawn |
| 183 | UserADBoard | `0x023d` | ShopSign |
| 201 | PetMove | `0x0282` | PetMove |
| 210 | UserMove | `0x029d` | PlayerMove |
| 211 | UserMeleeAttack | `0x02a8` | PlayerAttack |
| 223 | UserAvatarModified | `0x02b8` | AvatarModified |
| 284-287 | MobEnterField/LeaveField/ChangeController/Move | `0x03c9`,`0x03d4`,`0x03d5`,`0x03dc` | MobSpawn/Kill/Control/Move |
| 314 | NpcMove | `0x0456` | NpcMove |
| 322 | DropEnterField | `0x0471` | DropSpawn |
| 336 | ReactorEnterField | `0x0487` | ReactorSpawn |

**Ordering is preserved across all 13 anchors.** That is not something chance produces, and several anchors
(e.g. `ShopSign` -> `UserADBoard`) were named here independently, long before this table was consulted - so the
alignment is validated by our own prior work rather than assumed.

This makes the table a **hypothesis generator with testable predictions**: an unnamed opcode's position between
two anchors predicts which Kinoko entry it should be, and that entry predicts a payload shape we can check
against real captures. Findings so far (recorded in each opcode's `summary`, with their evidence and their
gaps):

- `0x022e` = **UserLeaveField** - at PlayerSpawn+1 exactly, as in Kinoko; payload `{int char_id}` = 6 bytes
  matches exactly. **This implies the existing name `PlayerLeave` on `0x0209` is on the wrong opcode** (0x0209
  is 13 bytes, which doesn't fit UserLeaveField; 58% of 0x022e events co-fire with a 0x0209 for the same char
  within 2s, so the two are halves of one leave). Needs a decision before renaming either.
- `0x02b9` = **UserEffectRemote** - at AvatarModified+1, as in Kinoko; the measured mode-byte-at-@6 shape is
  exactly an effect-type discriminator, which also explains why mode 1 mixes small ids and item-range ids.
- `0x02bb` = **UserTemporaryStatSet** - position fits; the measured 128-byte "all zero except flag bytes" field
  is exactly a temporary-stat bitmask.
- `0x02af` = **UserHit**, POSITION ONLY, weaker - content fits (player + mob oid + mob template), but the
  behavioural test (named mob should be adjacent to the player) was run with wrong position extraction and is
  inconclusive, not negative. Re-run it with `neon-world`'s own `end_position` before trusting it.

**Method to repeat for the remaining ~110:** place the unknown between its two nearest named anchors, read off
the Kinoko entries in that span, and test the predicted payload shape against captures. Cheap, zero risk, and
it does not need the client binary at all.

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

### RESULT (2026-10-27): the opcode -> handler mapping is NOT statically reachable

Attempted on jib01 (pid-of-the-day, image base `0x140000000`) with the tools below. Three independent lines of
evidence, all negative:

1. **The dispatch is virtualised.** `0x1416667a0` is four instructions long - a standard prologue, then
   `jmp 0x144c0d0f8`, which lands in the **rwxp** region `0x143ebd000-0x145277000` (not the plain `r-xp` image).
   The code there is textbook virtualiser output: redundant `push`/`pop` shuffles that cancel out
   (`mov [rsp],r9; sub rsp,8; mov [rsp],r9; mov r9,[rsp]; add rsp,8`), self-cancelling constant arithmetic
   (`sub QWORD PTR [rsp],0x6fff7969` immediately followed by `add r8,0x6fff7969`), and pushed VM keys
   (`0x7f774b79`, `0x76cef21f`, `0x7f8c1950`). This is the same protector wall that already blocked the
   per-connection wire-opcode numbering - now confirmed to cover the receive dispatch too.
2. **Scanning the plain image for an opcode immediate is pure noise** (`exescan.py`). `0x441` has 63 imm32 hits
   in the plain `r-xp` image; the ones that look promising (adjacent pairs 6 bytes apart) disassemble to
   byte-by-byte struct copies - `movzx eax,[rdi+0x440]; mov [rbx+0x440],al; movzx eax,[rdi+0x441]; ...` - where
   `0x441` is a *struct field offset*, not an opcode. A game struct has fields at every small offset, so every
   opcode number collides with one. imm16 is worse: 3047 hits.
3. **There is no static `{opcode -> handler}` table** (`exetable.py`). Searching every r-xp/r--p/rw-p region for
   an opcode value within 16 bytes of a pointer into the code image found 368 pairs, but every run of >=4 is in
   `.rdata` and decodes to `{pointer, small length}` string-table entries whose length field merely coincides
   with an opcode number (pointers target sequential *data* 16 bytes apart, not code). No table of handler
   addresses exists in readable memory.

**What this rules out:** step 2 of the ladder cannot disambiguate `0x0441`, `0x02af`, `0x022e`, `0x02b9`,
`0x0279`, `0x02bb` or anything else by reading the dispatch. Don't re-attempt it from the dispatch side - the
evidence above is reproducible with the three committed tools in minutes.

**What is NOT ruled out** (ideas that don't go through the dispatch, in rough order of promise):
- Anchoring on *game state* instead of packet bytes: `neon-stream` already knows static roots for HP/MP/level/
  position. Code that writes those addresses is, by definition, the tail of whatever handler processed the
  packet that changed them. Finding writers of a known address is a different search than finding opcode
  immediates, and is not obviously blocked.
- The protector virtualises selected functions, not all of them (it is far too slow). The handlers themselves
  may well be plain code - the problem is only that the *edge* from opcode to handler is hidden. Any other way
  to get a handler's address (a callback registered in a readable structure, an RTTI/vtable for a packet-handler
  class hierarchy, a log/assert string naming the packet type) would land inside plain code.
- Live tracing is the obvious answer and is **off the table**: a debugger attach has twice dropped both test
  accounts (`gdb-attach-drops-sessions`). Do not trade a session for this.

Workflow for one opcode (if a non-dispatch anchor is ever found):
1. From the dispatch table at `0x1416667a0`, find the case for this opcode's internal id (same table format as
   whatever found the existing addresses above - extend the same `objdump` + cross-reference method).
2. Dump and disassemble that handler.
3. Match disassembled offsets/comparisons against the byte ranges `scripts/correlate.py`/the explorer already
   flagged as "unexplained, varying" - a `cmp` against a small constant, a call into a known formatter, or a
   write to a struct field whose other uses are already known, is the behavioral evidence a name needs.
4. Write the finding into the opcode's `summary` with the exact addresses read, so it's reproducible and doesn't
   need re-deriving after the next client update.

## Tier B encoders: DONE for the whole registry (2026-10-27)

`Registry::encode_onto(dir, template, fields)` is now the exact inverse of `Registry::decode`, with
`tests/encode.rs` covering it and `cargo run --example rt_check -- <logs>` running it over live captures.

**It is patch-based on purpose.** For almost every opcode the registry explains only some of the bytes, so
synthesising a packet from field values alone would mean inventing the rest - guessing dressed up as protocol.
`encode_onto` instead takes a packet the server really sent and writes the named fields into it; the bytes we
do not understand are carried over from real traffic. That is exactly what Tier B replay needs (take a real
packet, change what you mean to change) and it is precisely round-trip testable.

Guard rails, all tested: an unknown field name is an error rather than a silent no-op; a value outside the
field's registered min/max is refused; a `str16` can only be replaced by a same-length string (a different
length would shift every later byte, including the unexplained ones).

**Validation result:** `1,777,788` real captured packets across ~40 opcodes decode and re-encode
**byte-for-byte identical, with 0 failures** - including the hard cases (`PlayerMove`'s repeated 21-byte
movement records, 446k packets; `PlayerSpawn`'s variable-length name field). This is also a strong independent
re-check of the registry itself: a field can be NAMED wrongly and still round-trip, but it cannot be
POSITIONED, sized or typed wrongly and round-trip.

What is still not possible: building a packet from nothing for an opcode whose bytes are not fully explained.
That needs the decode work this plan tracks, not more encoder work.

### Original plan for Tier B encoders (kept for context)

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
