# Disabling the anti-cheat (GRAP + NGS) before injecting

Goal: launch MapleStory Classic World with **no anti-cheat in the process** so the
injected probe/hook runs against plain, unprotected code ("same as 20 years ago").

## What the client actually depends on (evidence)

`MapleStory.exe` has exactly two static imports that belong to the anti-cheat:

```
DLL Name: grap64.dll          -> Ordinal 9 (no name; 9 ordinal-only exports, Themida-packed)
DLL Name: nmcogame64.dll      -> NMCO_MemoryFree (only named import)
```

Verified with `x86_64-w64-mingw32-objdump -p MapleStory.exe`; no other loaded
module imports `grap64.dll` / `nmco*.dll`, and `strings` shows no
`GetProcAddress` targets beyond those two names. So a mock only has to export
`grap64.dll!ordinal9` and `nmcogame64.dll!NMCO_MemoryFree` (plus the rest of the
nmco names, inert) for the game to start.

The `grap/` folder (`grap-core64.aes`, `grap-communicator64.aes`,
`BlackCat64.sys`) and `ProgramData\Nexon\NGS\NGService.exe` are the real service
and its kernel driver; they only get used if the real DLLs are loaded. This is
the community post's "mock dll that mimics ngs/blackcipher".

## What we ship

- `neon-probe/mock-grap64.c` / `.def` — exports ordinal 9, returns success.
- `neon-probe/mock-nmcogame64.c` / `.def` — inert `NMCO_*` stubs
  (`NMCO_MemoryFree` is a deliberate no-op, matching the real DLL's degraded
  behaviour when no NGS runtime registered its free callback).
- `neon-probe/hook-session.c` — IAT-hooks `CreateProcessW`/`A` in every module of
  the injected launcher process and copies the one-time session token out of the
  `MapleStory.exe` command line into `NeonSessionToken` shared memory.
- `scripts/mock-anticheat.sh install|restore|status|inject`.

Both mocks log every entry point they see to
`drive_c/tmp/neon-mock-*.log`; the launcher hook logs to
`drive_c/tmp/neon-session-hook.<pid>.log` and the captured token to shared memory.

## Usage

```bash
# 1. with the game CLOSED: swap the real DLLs for the mocks
scripts/mock-anticheat.sh install

# 2. with the launcher up, before PLAY: hook CreateProcessW in nexon_*
scripts/mock-anticheat.sh inject

# ... click PLAY as usual (scripts/relaunch.sh) and then inject the probe ...

scripts/mock-anticheat.sh restore   # put the real anti-cheat back
```

`install` never touches the game's `.orig` backups once made; `restore` copies
them back. Files are under
`drive_c/Nexon/Library/maplestorycw/appdata/`.

## Why a hook and not a debugger

Same reason as `neon-probe/DESIGN.md`: no ptrace, no thread stopping. The
anti-cheat mock removes protection; the launcher hook only rewrites the
launcher's own import slots (reversible, no code bytes touched).

## Reproduce / launch (macOS)

The launcher registers an `nxl:` URL scheme. Launch the Classic World product directly
instead of clicking PLAY (the click is flaky when other windows overlap):

```bash
open "nxl://launch/59813"     # 59813 = MapleStory Classic World (productId)
```

(seen product ids: `10100` = MapleStory, `59813` = MapleStory Classic World.)

Reproduce the current state: game closed, `scripts/mock-anticheat.sh install`, then the
`open` above. The game starts and shows its NGSX "game file has been damaged" dialog
(the dialog text is blank until the wz string table loads). Clicking OK currently exits
the client; baseline (both DLLs real) is clean. `scripts/lib-macos-ui.sh` needed fixes
for this launcher: the owning app is **"MapleStory Launcher"**, and a resident
`MapleStory Classic World.app` helper must not be mistaken for the game process.

## Live findings (2026-10-29)

- The launcher is **32-bit**, the game is **64-bit**. Injecting a 64-bit DLL into the
  launcher's `nexon_client.exe`/`nexon_runtime.exe` fails (wrong-arch `kernel32`
  `LoadLibraryW` -> access violation). Use the `inject32.exe`/`hook-session32.dll`
  builds; `scripts/mock-anticheat.sh inject` does.
- The launcher launches the game through the native macOS helper
  `MapleStory Classic World.app`, which then runs `wine MapleStory.exe` with the token
  on the command line as `-nxl NP12:us:0:<n>:<token>`. So on macOS the
  `CreateProcessW` hook may not fire for the game — `scripts/mock-anticheat.sh token`
  and `neon-loader --print-token` read it from the game process instead. The hook still
  matches the Linux/WineCharm path.
- MapleStory.exe is **Themida-packed** and validates its imports. A `grap64.dll` mock
  with the wrong export shape fails at startup with
  `Themida / An Error has occured while loading imports. Wrong DLL present.`.
  The mock must reproduce the real shape: export **base 1, nine NONAME ordinals**
  (ordinal 9 is the one the game imports). Fixed in `mock-grap64.def`; the game then
  reaches the login screen.
- With the grap mock in place the game shows its own "the game file has been damaged"
  check (file-integrity) at login. Still to resolve — likely a hash check against the
  patch manifest.
- The "damaged" dialog is an **NGSX** result: the client contains `CSecurityNGSX`,
  `NGS_Detect`, `NGS_Process`, `ngshash`/`ngshwid` strings and `GC:NGSXInitFailed`.
  A mock changes whatever NGS hashes, so the check fires.
- With both mocks the game stays up and shows the dialog; clicking OK currently exits
  the client. (An earlier "exits before login" was a launcher-timing artifact, not the
  mocks.)

## Status

- [x] `grap64.dll` mock passes Themida; game reaches login.
- [ ] Game-side "file damaged" check with a mock present — dismiss/accept or satisfy it.
- [ ] `nmcogame64.dll` mock: game stays up.
- [ ] No kick over a long session with the mocks + a probe injected.
- [x] `hook-session32.dll` loads and installs in all `nexon_*` processes without recursion.
- [ ] Token captured via `scripts/mock-anticheat.sh token` / shared memory on a real launch.
