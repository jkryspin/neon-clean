# neon-probe: how to find the handler for an opcode without patching a single code byte

## The problem (docs/decode-plan.md)
The receive dispatch at 0x1416667a0 jumps straight into the protector's virtualised region, so the
opcode -> handler edge is unreadable statically. Handlers themselves ARE plain code (all 80 virtual
methods of the stats object are readable). So: find which PLAIN code runs while a given packet is processed.

## Why not the obvious tools (each has already failed here, with evidence)
- inline hook / int3 at the dispatch: modifies code bytes - the most integrity-check-detectable thing a
  module can do, and the category that has already cost sessions.
- hardware breakpoints: the client occupies all four debug registers (perf_event_open -> ENOSPC).
- sampling profiler: ~5 kHz can't catch a microsecond handler (validated and rejected on NpcMove).
- gdb/ptrace: dropped both test accounts twice.

## The technique: PAGE_GUARD first-touch tracing
1. Keep the plain image RX the whole time. At the START of processing a packet, mark the plain code pages
   PAGE_GUARD. Guard is one-shot per page: the first instruction fetch from a page raises
   STATUS_GUARD_PAGE_VIOLATION and clears the guard.
2. A vectored exception handler (VEH) records (thread, faulting RIP, page) and lets execution continue.
   No code byte is written; only page protection bits change, and they are restored afterwards.
3. At the END of the packet, drop remaining guards (restore plain PAGE_EXECUTE_READ).
4. The ordered list of pages first-touched during one packet = a page-granular trace of its handler.
5. Repeat over many packets of each opcode and DIFF: pages touched by EVERY packet of opcode X but by
   packets of few other opcodes are that opcode's handler pages. Opcode-independent pages (allocator,
   memcpy, the dispatch plumbing itself) wash out in the diff.

## Open question that decides whether this works at all (answer it on a throwaway first)
How do we know when a packet starts and ends WITHOUT hooking? Candidates, least intrusive first:
 a. Use the network side we already have: tcpdump arrival timestamps from neon-stream give the packet
    boundaries; trace continuously and attribute guard faults to the nearest preceding arrival. No hook.
 b. Guard ONLY the dispatch page: the first touch of the page containing 0x1416667a0 marks "a packet is
    being dispatched" (the same function runs for every packet). That is a guard, not a patch.
Option (b) needs no cross-process clock alignment and no code patch. Use it.

## Hazards, stated up front
- Guard faults are per-process: OTHER threads touching a guarded page consume the one-shot guard, so a
  page can be "stolen" from the main thread. Filter by thread id; accept some misses; repeat sampling.
- A fault storm on a 51 MB image (13k pages) could stall the game thread long enough to miss a
  heartbeat. Cap pages guarded per packet, trace only a few packets per opcode, and measure the stall.
- VirtualQuery/VirtualProtect on those pages can itself be observed by a protector that watches its own
  region. We never touch the rwxp (protector) region - only the plain r-x image.
- Everything restores on unload. Verify with VirtualQuery before/after.
