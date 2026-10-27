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
# Native build (Linux)
make

# Cross-compile for Windows (requires mingw-w64)
make cross
```

## Usage

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
