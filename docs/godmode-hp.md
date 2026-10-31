# Godmode (HP lock): state and tooling — 2026-10-30

Goal: lock HP/MP from inside the client, so a bot level-0 character in Forest West of
Henesys cannot die while the panel watches. Nothing here patches game code bytes; the
module only reads/writes values and runs on its own thread.

## What works

- `neon-probe/inject-stealth.exe` manual-maps `neon-hack/neon-hack.dll` into a running
  `MapleStory.exe` and survives minutes of game time (module absent from PEB, MZ wiped,
  random base, no RWX, no new visible thread).
- The module's control channel (`Local\NeonHack.<pid>`) and `neon-win/hackctl.py`
  (`status|on|off|force <addr> <value>`) drive it from outside.
- Writing values works, provided the page is writable. The stat struct sits on a
  **read-only image page**, so the module now `VirtualProtect`s the target page to RW
  before the first write (`make_rw`, logged as `made <addr> writable`).
- Scanning memory from the tick thread is safe once guard pages are skipped
  (`writability_private` used to mask with `0xFF`, so `PAGE_GUARD` (0x100) slipped
  through and the first guarded region killed the client).

## Two client-killing bugs found and fixed

1. **CRT stdio in `DllMain`.** `_wfopen()` crashed the client ~immediately (marker file
   written, `hack.<pid>.log` never created, no WER entry). Logging is now raw
   `CreateFileW`/`WriteFile`; the DLL pulls no stdio in.
2. **No unwind tables.** A manual-mapped image is unknown to the unwinder, so the first
   access violation inside the DLL killed the process instead of running our
   `__except`. `inject-stealth.exe` now registers the DLL's `.pdata` with
   `RtlAddFunctionTable` from the shellcode (it must run there: the image headers are
   wiped before `DllMain`).

## Open puzzle: where HP lives

Read-only material: `docs/data-sources.md`, `neon-calib/chains.json`.

- The static `stats_object` root (`0x143bf8e78`) and the position root
  (`0x143bf05a0`) read **0** in every Windows session tried, although the exe hash is
  the calibrated `1942A7B3…`, the module base is `0x140000000`, `.data` matches the file
  and no remapped copy of the image exists. `neon-stream --verify` will therefore report
  nothing useful here.
- Blind search results that are real but *not* the HP field:
  - adjacent `u32` pairs `(x, max_hp)` — 48 private hits, writing them does not move the
    bar;
  - `u16` pairs `(162,162)` — 2 hits, static (client never rewrites them);
  - a heap object with level 0 and `42` at `+0x360` that matched the bar once.
- **Writing HP out is invisible on the HUD.** The bar and the `NNN/mmm` text only move
  when a server stat packet arrives, so a local write can never be validated by looking
  at the screen — only by surviving damage.
- **Forest West of Henesys barely damages in these sessions.** Stand-still and
  walk-through runs of 1–2 minutes often take zero hits, which starves every
  value-differential method. Even potions are now exhausted.

### Tools left in the repo for the next attempt

`neon-hack/scanout.exe` (external reader, safe to run, never writes unless asked):

```
scanout <pid> read                 # walk + read everything
scanout <pid> find <a> <b>         # adjacent u32 pair
scanout <pid> findv <v>            # equal u32
scanout <pid> findmax <m>          # (x,m) pairs
scanout <pid> findmaxp <m>         # same, MEM_PRIVATE only
scanout <pid> findseq <a> <b> <c> <d>   # u16 sequence (stat block)
scanout <pid> findrange <lo> <hi>  # any u32 in range
scanout <pid> findchain <o1> <o2> <a1> <a2> <b1> <b2>  # pointers from the image to
                                                       # objects with level 0 at +0x160
                                                       # and fields in the given ranges
scanout <pid> findblob <file.hex>  # byte pattern
scanout <pid> bigregions           # regions >= 32 MB
scanout <pid> peek|peek64|write|diffcheck <listfile>
```

Useful and cheap: `neon-win/hackctl.py <pid> force <addr> <value>` writes every 100 ms
from inside the game (this is the godmode write path once the address is known).

### Recommended next step

1. Take the character somewhere that damages reliably ("A Hill West of Henesys", the
   hunting ground at `x≈1625`, per `docs/data-sources.md`).
2. `findchain 0x360 0x364 20 200 1 3000`, take a hit, and re-read the candidates'
   `+0x360` immediately — the field that tracks the bar is HP.
3. `hackctl force <addr+0x360> <max>` and confirm the bar stops falling while hits
   (damage numbers) continue.

The module already carries the plumbing for a fast lock (`force` = 100 ms tick, RO-page
flip, SEH-safe writes) so step 3 is one command.
