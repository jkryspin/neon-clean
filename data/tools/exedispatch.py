#!/usr/bin/env python3
"""Find the client's packet-dispatch site(s) by looking for where MANY DISTINCT known opcodes cluster together.

  neon-calib/tools/exedispatch.py [PID] [--window=N] [--min=N]

Scanning for one opcode value is hopeless (exescan.py on 0x441 returned 63 imm32 hits, nearly all struct-copy
loops where 0x441 is a field offset, not an opcode). But a dispatch - whether a switch jump table, an if/else
comparison chain, or a static table of {opcode, handler} pairs - is the one place where MANY different opcode
numbers appear within a small address range. That is a signal chance can't fake: ~160 known opcode values,
clustered 10+ distinct within a few hundred bytes.

Reads /proc/PID/mem by seek. No ptrace, no debugger attach (see exeread.sh for why that rule exists).
Report only - prints candidate clusters to disassemble with exeread.sh.
"""
import collections, re, struct, subprocess, sys

args = [a for a in sys.argv[1:] if not a.startswith("--")]
WINDOW = int(next((a.split("=")[1] for a in sys.argv if a.startswith("--window=")), 256))
MIN_DISTINCT = int(next((a.split("=")[1] for a in sys.argv if a.startswith("--min=")), 8))

pid = int(args[0]) if args else None
if pid is None:
    for line in subprocess.run(["bash", "-c", "pgrep MapleStory.exe"], capture_output=True, text=True).stdout.split():
        if "jib01" in open(f"/proc/{line}/maps", errors="replace").read():
            pid = int(line)
            break
    if pid is None:
        sys.exit("no jib01 client found; pass a PID explicitly")

# Every opcode the registry knows, both directions - the dispatch should reference a lot of them.
text = open("neon-protocol/opcodes.toml").read()
opcodes = sorted({int(m, 16) for m in re.findall(r"^id = (0x[0-9a-f]+)", text, re.M)})
# Drop values small enough to collide with ordinary small constants/offsets everywhere in the image.
opcodes = [o for o in opcodes if o >= 0x20]
print(f"pid {pid}: looking for clusters of {len(opcodes)} known opcode values "
      f"(window {WINDOW} B, >= {MIN_DISTINCT} distinct)")

regions = []
for line in open(f"/proc/{pid}/maps"):
    m = re.match(r"([0-9a-f]+)-([0-9a-f]+) (\S{4})", line)
    if m and m[3] == "r-xp" and 0x140000000 <= int(m[1], 16) < 0x146000000:
        regions.append((int(m[1], 16), int(m[2], 16)))

by_value = {struct.pack("<I", o): o for o in opcodes}
found = []          # (addr, opcode)
mem = open(f"/proc/{pid}/mem", "rb", buffering=0)
for lo, hi in regions:
    mem.seek(lo)
    try:
        buf = mem.read(hi - lo)
    except OSError:
        continue
    for needle, op in by_value.items():
        start = 0
        while True:
            i = buf.find(needle, start)
            if i < 0:
                break
            found.append((lo + i, op))
            start = i + 1

found.sort()
print(f"{len(found)} total imm32 occurrences of known opcode values")

# Slide a window over the sorted hits and report ranges holding many DISTINCT opcodes.
clusters = []
for i, (addr, _) in enumerate(found):
    j = i
    seen = set()
    while j < len(found) and found[j][0] - addr <= WINDOW:
        seen.add(found[j][1])
        j += 1
    if len(seen) >= MIN_DISTINCT:
        clusters.append((len(seen), addr, found[j - 1][0], sorted(seen)))

# Keep only the best (most distinct) cluster per overlapping area.
clusters.sort(key=lambda c: -c[0])
kept = []
for c in clusters:
    if all(abs(c[1] - k[1]) > WINDOW for k in kept):
        kept.append(c)

print(f"\n{len(kept)} candidate dispatch cluster(s):\n")
for n, start, end, ops in kept[:25]:
    print(f"  {start:#x}..{end:#x}  {n} distinct opcodes: {', '.join(f'{o:#06x}' for o in ops[:14])}"
          + (" ..." if len(ops) > 14 else ""))
if not kept:
    print("  none - the dispatch is probably a jump table of ADDRESSES indexed by opcode (no opcode values\n"
          "  stored at all), or it lives in the protector's virtualised region. Next: look for an array of\n"
          "  pointers into the r-xp range instead of opcode immediates.")
