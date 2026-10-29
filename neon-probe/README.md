# Neon Probe - Opcode Mapping Tools

## Overview

Tools for mapping wire opcodes to handler addresses in the MapleStory Classic World client.

## Files

- `inject.c` - Standard DLL injector (CreateRemoteThread + LoadLibraryW)
- `inject-undetectable.c` - Undetectable injector (thread hijacking + manual PE mapping)
- `probe.c` - Basic observer DLL (logs memory regions)
- `probe-opcodes.c` - Opcode mapping DLL (logs opcode → handler mappings)
- `Makefile` - Build system for native and cross-compiled targets

## Building

```bash
# Windows injector + probe DLL (mingw-w64) - what you actually run
make inject.exe probe_opcodes-win.dll

# Native build (Linux, gcc)
make

# Cross-compile for Windows (requires mingw-w64)
make cross
```

## Usage

### macOS (this Mac, "MapleStory Launcher.app")

The bundled Wine is x86_64-on-ARM64: the unix side is native arm64, the PE side
runs under Rosetta. There is no usable cross-process route through Mach APIs
(x86 thread state is hidden from arm64 callers and the x86 address space is not
reachable via mach_vm - the reason `inject-macos` and this morning's PE-scan
tools hit a wall; `dump-regions.c` shows what a Rosetta task looks like from
the host).

The working path is the same as on Linux: run `inject.exe` as a Wine process in
the same bottle and let wineserver do the work (OpenProcess, WriteProcessMemory,
CreateRemoteThread are serviced internally - no debugger, no ptrace, nothing
that changes the target's code bytes on the way in).

```bash
# 1. Launch the game from "MapleStory Launcher.app" (or it is already running).
# 2. Inject:
cd neon-probe
./inject.sh MapleStory.exe C:\Neon\probe_opcodes-win.dll

# 3. Watch the log
tail -f "$HOME/Library/Application Support/MapleStoryNA/Bottles/maplestory/drive_c/tmp/neon-probe-opcodes."*.jsonl
```

`inject.sh` copies the dll to `C:\Neon` in the bottle and passes the Windows
path through. Test the plumbing without the game first:

```bash
./inject.sh cmd.exe ./hello-win.dll     # writes C:\Neon\hello.log
./inject.sh cmd.exe --unload hello-win.dll
```

Gotchas learned on this machine:
- `LoadLibrary` refcounts: injecting a DLL that is already loaded returns the
  existing HMODULE and does NOT run DllMain again. Repeat `--unload` until the
  injector says the module is not found before expecting a fresh DllMain.
- Win32 pids differ from macOS pids; pass the image name (`MapleStory.exe`) or
  the pid shown by `inject.exe` (a `tasklist`-style number from wineserver).
- `C:\Neon` in the bottle must be writable by the user, or a probe's log write
  fails silently inside DllMain.

### 1. Launch the game

```bash
wine MapleStory.exe
# Note the PID
```

### 2. Inject the opcode probe

```bash
# Using undetectable injector
wine inject_ud-win.exe <pid> probe_opcodes-win.dll

# Or using standard injector
wine inject-win.exe <pid> probe_opcodes-win.dll
```

### 3. Watch the log

```bash
tail -f /tmp/neon-probe-opcodes.<pid>.log
```

The log will show:
- When the probe loads
- Memory regions it can see
- Each opcode as it's received with the handler address
- Summary when unloaded

### 4. Generate traffic

In the game, perform various actions:
- Move around
- Attack mobs
- Use skills
- Chat
- Open/close windows

This generates different opcodes that the probe will log.

### 5. Unload the probe

```bash
wine inject_ud-win.exe <pid> --unload probe_opcodes-win.dll
```

### 6. Review the summary

The log file will contain a summary of all observed opcodes:
```
=== OPCODE SUMMARY ===
Total unique opcodes: XX
0x0441: handler=0x1416667a0 count=150
0x02af: handler=0x141666800 count=75
...
```

## Known Opcodes

| Opcode | Description | Handler |
|--------|-------------|---------|
| 0x0441 | UserHit (attack) | TBD |
| 0x02af | NpcMove | TBD |
| 0x022e | Unknown (high traffic) | TBD |
| 0x02b9 | Unknown (high traffic) | TBD |
| 0x0279 | Unknown (high traffic) | TBD |
| 0x02bb | Unknown (high traffic) | TBD |

## Notes

- The probe is an observer only - it doesn't modify game code
- It uses passive scanning to find the dispatch table
- Output goes to `/tmp/neon-probe-opcodes.<pid>.log`
- Works under Wine on Linux

## Next Steps

1. Run the probe on a test account (shurella2)
2. Generate various types of traffic
3. Collect opcode → handler mappings
4. Use the mappings to build encoders for Stage B
