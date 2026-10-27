# In-process instrumentation ("the injector"): goal, value, risk, and gates

Status: **PROPOSAL ONLY. Nothing here has been built or run.** It exists so the decision to modify the game
process is made on written reasons, not momentum. Written 2026-10-27 after the passive/static routes were
exhausted (see `docs/decode-plan.md`).

## 0. Terminology - two unrelated things are both called "injection"

| | **Stage B packet injection** | **In-process injection / instrumentation (this document)** |
|---|---|---|
| What | The bot sends crafted packets on the network, headless | Code is loaded into, or the memory of, the running game client is modified |
| Touches the game process? | No | **Yes** |
| Authorised? | **Yes** - `docs/authorization.md`, Stage B (NP12-T first) | **No - see section 5** |
| Purpose | Play without the client | Learn what the client does with each packet |

Everything in the Stage B approval is about the first column. This document is only about the second.

## 1. The goal, in one sentence

Find out **which client function handles each opcode and what it does with each byte**, for the opcodes where
no other method can tell us.

That is a different kind of evidence from everything collected so far. Passive methods answer *when* a packet
occurs and *what it is correlated with*. Only the handler's own code answers *what the bytes mean*.

## 2. What it would deliver (concrete, checkable)

1. **An opcode -> handler address map** for the incoming opcodes, which the protector hides statically
   (the receive dispatch at `0x1416667a0` jumps straight into the virtualised `rwxp` region).
2. **For each handler, the bytes it actually reads**: field offsets, sizes, and which are compared against
   what, which are passed to a formatter, which are stored into a game object whose field meaning is already
   known (HP `+0x360`, MP `+0x364`, level `+0x160` on the stats object).
3. A **definitive** name for the remaining unnamed opcodes, instead of the current "probably, by position and
   behaviour".

It would NOT, by itself, deliver: a way to build packets from nothing (that needs every byte explained, and
unexplained bytes may be ignored by the client entirely), or any server-side knowledge (what the server does
with what we send).

## 3. What it would NOT change (read this before deciding)

**The bot's actual needs are already met.** Position, mobs and their templates, drops, EXP, HP/MP, other
players, attacks, skill use, chat, reactors, party, inventory: all decoded, validated, and round-trip tested
(2,031,664 real packets byte-for-byte). The authorised purpose is price-data collection
(`docs/authorization.md`, "Purpose: Price data collection for MapleWatch"). None of the 116 remaining unnamed
opcodes is known to be needed for that.

So injection buys **completeness of the protocol map**, not capability the bot currently lacks. That is a
legitimate engineering goal (the stated aim is "the traffic can be replayed ... including processing and
reply of all the opcodes"), but it should be weighed as completeness, not necessity.

Equally, 91 of the 139 unnamed opcodes (incoming + outgoing) have **never been seen at all** in 14+ hours of
traffic. Injection cannot explain a packet that never arrives; for those the limiting factor is a trigger, not
analysis.

## 4. Why cheaper routes are exhausted (evidence, not assertion)

| Route | Result |
|---|---|
| Static: read the dispatch | Virtualised - 4 instructions then `jmp` into the protector's `rwxp` region; self-cancelling push/pop, arithmetic that undoes itself, pushed VM keys |
| Static: scan for opcode immediates | Pure noise - `0x441` has 63 imm32 hits, nearly all struct-copy loops where it is a field *offset* |
| Static: find a handler table | No readable `{opcode -> handler}` table. MSVC x64 jump tables (4-byte RVAs) searched too; large candidates were an FFT bit-reversal table and bit-masks, not a switch |
| Static: RTTI | Class names survive (446) but the protector relocated RTTI past the image end, so the COL -> vtable linkage does not resolve |
| Static: debug opcode-name table | None in this build (no `kc_PACKET`/`ks_PACKET`) |
| Dynamic, passive: perf sampling | Too coarse - validated on the *known* `NpcMove`: 162 arrivals, no address enriched 3x |
| Dynamic, passive: Intel PT / uprobes | No Intel PT (AMD CPU); game code is anonymous memory with no backing file |
| Dynamic, passive: hardware watchpoints | **Blocked by the anti-cheat** - all four debug registers are already occupied (ENOSPC); an identical call on a control process succeeds |
| Public prior art (Kinoko alignment) | Works, and has named several opcodes - but gives *which slot*, not *what the bytes mean*, and cannot be confirmed by code |
| Client data files (WZ) | Excellent for id fields (found `skill_id@8`); finds nothing in the remaining state/flag broadcasts |

What static analysis *did* establish: game classes' own **methods are plain readable code** (all 80 virtual
methods of the stats object); only the opcode->handler *edge* is hidden. That is what makes a dynamic trace
worthwhile at all - it would land in readable handler code.

## 5. Scope and authorisation - the real blocker

`docs/authorization.md`, the technical design disclosed to Nexon:

> "Memory Access: process_vm_readv ... or ReadProcessMemory"
> "**No memory is written directly** - all actions use input simulation ... or packet injection through the
> normal game client."

and Nexon's reply: "we just want to make sure it's not doing anything too aggressive with packet injection."

In-process injection **writes to or loads code into the game process**. That contradicts the design as
described and approved. "Feel free to implement whatever anti-detection you need" concerns *detection*, not
modifying the client, and is not a reasonable basis for assuming consent to instrument it.

The anti-cheat evidence points the same way: it is actively occupying debug registers, which means it is
looking for exactly this. Defeating that is not "reading memory".

**Recommendation: do not proceed without either (a) asking Nexon (Kevin Song, the approver of
MSCW-BOT-AUTH-2026-1023-KS) whether client instrumentation on test accounts is acceptable, or (b) the account
owner making an explicit, informed decision to exceed the disclosed scope, accepting the account risk.**
That is the user's call, not mine; this document only records that the question exists.

## 6. Risks

- **Account loss.** A debugger attach already dropped both test accounts to the login screen twice
  (`gdb-attach-drops-sessions`). In-process code is a stronger signal than ptrace.
- **Integrity checks.** Patching code bytes is detectable by checksum; the protector very likely has them.
  Observation-only designs avoid this, patching does not.
- **Authorisation breach** (section 5) - potentially affecting the whole Nexon arrangement, not just one
  account; authorisation is revocable on 30 days' notice.
- **Data contamination.** An instrumented client that stalls or desyncs corrupts the capture pipeline
  (`docs/mapping-workflow.md`, the jib02 incident). Capture must be validated before and after.

## 7. Candidate designs, least to most invasive

Pick the lowest rung that answers the question; stop as soon as it does.

1. **Host-side only (Wine, not the game).** The client runs under Wine, which is open source. Wine mediates
   the game's attempts to set debug registers; a modified Wine could stop honouring them, freeing DR0-DR3 so
   `perf -e mem:ADDR:w` works (the "who writes the HP field" question). The game's own memory and code stay
   byte-identical. Cost: building/maintaining a patched Wine; and the anti-cheat may notice that its debug
   registers did not stick. *Least invasive to the game, still an evasion of an anti-debug measure.*
2. **Observation-only in-process trace** (e.g. Frida Stalker following only the main thread, recording basic
   blocks, never writing game code). Detectable by its own presence (it injects via ptrace) even if it never
   alters game bytes.
3. **Targeted logging stub** at a handler found by 1 or 2. Requires writing code bytes - the integrity-check
   risk. Last resort.

Rungs 2 and 3 need section 5 settled first. Rung 1 arguably does too.

## 8. Gates (all must be true; any "no" means stop)

1. Section 5 resolved in writing.
2. **Test account only**, never the account that feeds MapleWatch. As of writing none qualifies cleanly:
   `jib02` is suspended (`T`), `jib01` is not in-game (null stats root, position 0,0), so a test account would
   first have to be put into combat.
3. **Baseline recorded** immediately before: process alive and running, TCP count, `heuristic_skipped`,
   HP/position sane, capture log growing.
4. **Validate the method on a KNOWN opcode first** (`NpcMove`/`StatUpdate`) - the way the sampling method was
   tested and rejected - so failure means "method too weak", not "opcode mysterious".
5. **Short, bounded runs** with health re-checked after each (seconds, not minutes), user watching.
6. **Abort immediately** on: session drop, `heuristic_skipped` rising, the process leaving state `S/R`, TCP
   count falling, or any unexplained client behaviour.
7. Every finding recorded with the exact addresses read, so it never needs re-deriving after a client update.

## 9. Success criteria

- Method validated: the trace recovers `NpcMove`'s handler and the field offsets already known for it.
- At least one previously unnamed opcode's handler identified with its read-offsets matching the byte ranges
  already flagged "unexplained, varying".
- Zero session loss and zero capture contamination throughout.

If the validation in gate 4 fails, stop: the method does not work here and further runs only add risk.

## 10. What to do instead, or first

These reach part of the same goal with no modification of the game and should come first regardless:

- **Put a test account into combat.** The biggest unknown (`0x0441`, 142k packets) only comes from the main
  account; the others are idle. Better data helps every method already built.
- **Keep compounding the Kinoko alignment.** Each confirmed opcode is a new anchor that narrows its
  neighbours - already took `0x02b9` to exactly one candidate, confirmed independently by the client's own
  skill data.
- **Decide how complete "complete" needs to be.** If the end goal is replaying the *authorised* bot's traffic,
  the work is essentially done. If it is a full protocol map for its own sake, injection is the remaining
  lever and sections 5-8 apply.

## 11. Open decisions (for the account owner)

1. Does the goal justify exceeding the disclosed "no memory is written" design, and is Nexon being asked first?
2. If yes, is rung 1 (patched Wine) acceptable as the first step, given it leaves the game byte-identical?
3. Which account is the sacrificial one, and does it need to be in combat?

## 12. Pre-existing anomaly to resolve before any invasive step

`shurella2`'s `heuristic_skipped` read 12 for the entire session and then read **24** right after read-only
`/proc/PID/mem` reads of that client's stats object. Passive reads should not affect the network capture, and
it may be coincidence (a reconnect or map change), but it is the first change all night and the cause has NOT
been established. Whether it is ongoing or a one-off, and whether it correlates with anything done, must be
checked first: a baseline that moved for an unknown reason is not a baseline.
