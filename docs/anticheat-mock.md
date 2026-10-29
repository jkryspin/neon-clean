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

## Status / what to verify live

- [ ] `grap64.dll` mock: game starts and reaches login (ordinal-9 return value
      may need adjusting — the log shows whether it is called and how often).
- [ ] `nmcogame64.dll` mock: no `NMCO_*` crash; game reaches login.
- [ ] No kick over a long session with the mocks + a probe injected.
- [ ] `hook-session.dll` in `nexon_client.exe`: token captured on PLAY
      (`NeonSessionToken` / the session-hook log).

If the game refuses to start with a mock, the first suspect is the ordinal-9
stub's return value; the real `grap64.dll` export table is unreadable statically
(Themida), so this is a live-iterate value.
