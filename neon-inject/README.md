# Neon Inject

DLL injection-based memory reader for MapleStory.

## Build

### Cross-compile for Windows (from Linux)

```bash
# Install Windows target
rustup target add x86_64-pc-windows-msvc

# Install mingw for cross-compilation
sudo apt install gcc-mingw-w64-x86-64

# Build
cargo build --release --target x86_64-pc-windows-gnu
```

### Build on Windows

```bash
cargo build --release
```

## Usage

### Calibrate Offsets

First, calibrate to find the correct memory offsets:

```bash
# Run MapleStory, stand still with known stats
# Then run calibrate:
./target/x86_64-pc-windows-gnu/release/calibrate.exe ^
    --pid <MapleStory_PID> ^
    --dll ./target/x86_64-pc-windows-gnu/release/neon_inject.dll ^
    --level 7 --hp 99 --max-hp 146 --mp 77 --max-mp 77
```

This will:
1. Inject the DLL into MapleStory
2. Search for your known values
3. Calculate offsets from module base
4. Save to `neon-offsets.json`

### Inject and Stream

```bash
./target/x86_64-pc-windows-gnu/release/inject.exe ^
    --pid <MapleStory_PID> ^
    --dll ./target/x86_64-pc-windows-gnu/release/neon_inject.dll ^
    --stream
```

### Update Offsets in Running DLL

```bash
# After calibration, update the DLL with new offsets
# (requires modifying the DLL or passing offsets at runtime)
```

## Output

### neon-offsets.json

```json
{
  "version": "1.0",
  "module_name": "MapleStory",
  "offsets": {
    "position": 37448,
    "map_id": 35736,
    "level": 33928,
    "hp": 37448,
    "max_hp": 37452,
    "mp": 37456,
    "max_mp": 37460
  }
}
```

## How It Works

1. **DLL Injection**: `neon_inject.dll` is loaded into MapleStory process
2. **Direct Memory Access**: DLL reads memory at known offsets from module base
3. **Stable Offsets**: Offsets are relative to module base, not absolute addresses
4. **Exported Functions**: DLL exports `get_player_data()` for reading stats

## Architecture

```
┌──────────────────────┐
│  calibrate.exe       │
│  - Injects DLL       │
│  - Searches for      │
│    known values      │
│  - Calculates offsets│
└──────────┬───────────┘
           │
           ▼
┌──────────────────────┐
│  MapleStory.exe      │
│  ┌────────────────┐  │
│  │ neon_inject.dll│  │
│  │ - DllMain()    │  │
│  │ - get_player_  │  │
│  │   data()       │  │
│  └────────────────┘  │
└──────────────────────┘
```

## Benefits

| Feature | DLL Injection | Memory Scanning |
|---------|--------------|-----------------|
| Address stability | ✅ Relative offsets | ❌ Absolute addresses |
| Permission issues | ✅ None | ❌ /proc/pid/mem |
| Performance | ✅ Direct read | ❌ File I/O |
| Can hook functions | ✅ Yes | ❌ No |
| Cross-platform | ❌ Windows only | ✅ Linux/Windows |
