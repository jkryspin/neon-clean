#!/usr/bin/env python3
"""Scan a live client's PLAIN (non-virtualised) code for references to an opcode number, to find its handler.

  neon-calib/tools/exescan.py OPCODE [PID]        e.g. exescan.py 0x441

Why: the receive dispatch at 0x1416667a0 immediately jumps into the protector's rwxp region (0x143ebd000+,
virtualised - redundant push/pop shuffles, self-cancelling arithmetic), so the dispatch itself cannot be read
statically. But virtualising every packet handler would be far too slow, so the handlers themselves are expected
to live in the plain r-xp image. This looks for the opcode value as an immediate in that plain code - a
`cmp reg, 0x441` style test, or the value embedded in a jump/dispatch table - which is where a handler
identifies itself.

Reads /proc/PID/mem by seek, no ptrace, no debugger attach (see exeread.sh's note on why that matters).
Report only; prints candidate addresses to disassemble with exeread.sh.
"""
import re, struct, sys

op = int(sys.argv[1], 0)
pid = int(sys.argv[2]) if len(sys.argv) > 2 else None
if pid is None:
    import subprocess
    for line in subprocess.run(["bash", "-c", "pgrep MapleStory.exe"], capture_output=True, text=True).stdout.split():
        maps = open(f"/proc/{line}/maps", errors="replace").read()
        if "jib01" in maps:
            pid = int(line)
            break
    if pid is None:
        sys.exit("no jib01 client found; pass a PID explicitly")

regions = []
for line in open(f"/proc/{pid}/maps"):
    m = re.match(r"([0-9a-f]+)-([0-9a-f]+) (\S{4})", line)
    if not m:
        continue
    lo, hi, perm = int(m[1], 16), int(m[2], 16), m[3]
    # plain executable image only: r-xp. The rwxp region is the protector's - scanning it finds only VM noise.
    if perm == "r-xp" and 0x140000000 <= lo < 0x146000000:
        regions.append((lo, hi))

print(f"pid {pid}, scanning {len(regions)} plain r-xp region(s) for opcode {op:#06x}")

# How the opcode appears as an immediate operand, most-specific first.
needles = {
    f"imm32 {op:#x} (cmp/mov r32)": struct.pack("<I", op),
    f"imm16 {op:#x} (cmp r16)": struct.pack("<H", op),
}

mem = open(f"/proc/{pid}/mem", "rb", buffering=0)
hits = {k: [] for k in needles}
for lo, hi in regions:
    mem.seek(lo)
    try:
        buf = mem.read(hi - lo)
    except OSError:
        continue
    for label, needle in needles.items():
        start = 0
        while True:
            i = buf.find(needle, start)
            if i < 0:
                break
            addr = lo + i
            # an imm16 hit inside an imm32 hit is the same site; only keep imm16 when the next 2 bytes aren't 0
            if label.startswith("imm16") and buf[i + 2:i + 4] == b"\x00\x00":
                start = i + 1
                continue
            hits[label].append(addr)
            start = i + 1

for label, addrs in hits.items():
    print(f"\n{label}: {len(addrs)} hit(s)")
    for a in addrs[:40]:
        print(f"  {a:#x}")
    if len(addrs) > 40:
        print(f"  ... {len(addrs)-40} more")
print("\nDisassemble a candidate with: neon-calib/tools/exeread.sh <addr-0x20> 160")
