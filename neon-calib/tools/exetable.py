#!/usr/bin/env python3
"""Look for a {opcode -> handler} dispatch TABLE: an opcode value sitting next to a pointer into the code image.

  neon-calib/tools/exetable.py [PID]

Rationale: scanning for opcode values alone is useless in this binary - exescan.py/exedispatch.py both drowned in
struct-offset noise (a game struct has fields at 0x441, 0x8b, 0x90... just like opcode numbers). But a real
dispatch table pairs each opcode with a HANDLER ADDRESS, and a pointer into 0x140000000-0x146000000 is a
signature that struct-copy code never produces. So: find every 8-byte value that is a plausible code pointer,
then check whether a known opcode number sits within a few bytes of it.

Also scans the read-only data regions (not just r-xp), since a dispatch table is usually in .rdata.
Reads /proc/PID/mem by seek - no ptrace, no debugger attach.
"""
import re, struct, subprocess, sys

args = [a for a in sys.argv[1:] if not a.startswith("--")]
pid = int(args[0]) if args else None
if pid is None:
    for line in subprocess.run(["bash", "-c", "pgrep MapleStory.exe"], capture_output=True, text=True).stdout.split():
        if "jib01" in open(f"/proc/{line}/maps", errors="replace").read():
            pid = int(line)
            break
    if pid is None:
        sys.exit("no jib01 client found; pass a PID explicitly")

text = open("neon-protocol/opcodes.toml").read()
opcodes = {int(m, 16) for m in re.findall(r"^id = (0x[0-9a-f]+)", text, re.M)}
opcodes = {o for o in opcodes if o >= 0x20}

CODE_LO, CODE_HI = 0x140001000, 0x146000000

regions = []
for line in open(f"/proc/{pid}/maps"):
    m = re.match(r"([0-9a-f]+)-([0-9a-f]+) (\S{4})", line)
    if m and m[3] in ("r-xp", "r--p", "rw-p") and 0x140000000 <= int(m[1], 16) < 0x146000000:
        regions.append((int(m[1], 16), int(m[2], 16), m[3]))

print(f"pid {pid}: scanning {len(regions)} region(s) for opcode-next-to-code-pointer pairs")
mem = open(f"/proc/{pid}/mem", "rb", buffering=0)
pairs = []
for lo, hi, perm in regions:
    mem.seek(lo)
    try:
        buf = mem.read(hi - lo)
    except OSError:
        continue
    # walk 8-byte aligned slots looking for code pointers, then look for an opcode value nearby
    for off in range(0, len(buf) - 8, 8):
        ptr = struct.unpack_from("<Q", buf, off)[0]
        if not (CODE_LO <= ptr < CODE_HI):
            continue
        for d in (-8, -4, 8, 12, 16):
            o2 = off + d
            if 0 <= o2 <= len(buf) - 4:
                v = struct.unpack_from("<I", buf, o2)[0]
                if v in opcodes:
                    pairs.append((lo + off, ptr, v, d, perm))
                    break

print(f"{len(pairs)} opcode/code-pointer pair(s)")
# A real table is a RUN of these at a regular stride - report runs, not isolated coincidences.
pairs.sort()
runs, cur = [], []
for p in pairs:
    if cur and p[0] - cur[-1][0] <= 32:
        cur.append(p)
    else:
        if len(cur) >= 4:
            runs.append(cur)
        cur = [p]
if len(cur) >= 4:
    runs.append(cur)

print(f"\n{len(runs)} run(s) of >=4 consecutive pairs (a real table should appear here):\n")
for r in sorted(runs, key=lambda r: -len(r))[:20]:
    ops = sorted({p[2] for p in r})
    print(f"  {r[0][0]:#x}..{r[-1][0]:#x}  {len(r)} entries, {len(ops)} distinct opcodes ({r[0][4]}): "
          + ", ".join(f"{o:#06x}" for o in ops[:12]) + (" ..." if len(ops) > 12 else ""))
if not runs:
    print("  none. Combined with the dispatch at 0x1416667a0 jumping straight into the protector's rwxp region,\n"
          "  this says the opcode->handler mapping is not a readable static table: it is inside the virtualised\n"
          "  code. Static analysis cannot reach it; see docs/decode-plan.md for what that rules out.")
