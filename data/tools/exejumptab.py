#!/usr/bin/env python3
"""Find MSVC x64 SWITCH JUMP TABLES in the client's readable code - the likely home of per-pool packet dispatch.

  neon-calib/tools/exejumptab.py [PID] [--min=N]

Why this exists: exetable.py searched for 8-byte code POINTERS next to opcode values, found nothing real, and
that was recorded as "no static dispatch table exists". That conclusion was too strong. MSVC x64 does not store
switch tables as 8-byte pointers: it stores 32-bit IMAGE-RELATIVE offsets (RVAs), and the code does
    lea  rdx,[image_base]          ; or a rip-relative base
    mov  eax,[rdx+rcx*4+TABLE]     ; load the 4-byte offset
    add  rax,rdx                   ; rebase
    jmp  rax
so a per-pool `OnPacket` switch (CUserPool / CMobPool / CNpcPool ...) is a run of u32 RVAs, not pointers.

What it looks for: runs of >= N consecutive 4-byte values that are ALL valid code RVAs (inside the plain
r-xp image) and that are clustered - a real table's targets are the case bodies of one function, so they sit
within a few KB of each other, which random data essentially never does for 8+ consecutive words.

Reads /proc/PID/mem by seek. No ptrace, no debugger, no writes to the target.
"""
import re, subprocess, sys
import numpy as np

IMAGE_BASE = 0x140000000
args = [a for a in sys.argv[1:] if not a.startswith("--")]
MIN_RUN = int(next((a.split("=")[1] for a in sys.argv if a.startswith("--min=")), 8))
SPREAD = int(next((a.split("=")[1] for a in sys.argv if a.startswith("--spread=")), 0x4000))

pid = int(args[0]) if args else None
if pid is None:
    for line in subprocess.run(["bash", "-c", "pgrep MapleStory.exe"], capture_output=True, text=True).stdout.split():
        if "jib01" in open(f"/proc/{line}/maps", errors="replace").read():
            pid = int(line)
            break
    if pid is None:
        sys.exit("no jib01 client found; pass a PID explicitly")

code_ranges, data_regions = [], []
for line in open(f"/proc/{pid}/maps"):
    m = re.match(r"([0-9a-f]+)-([0-9a-f]+) (\S{4})", line)
    if not (m and 0x140000000 <= int(m[1], 16) < 0x146000000):
        continue
    lo, hi, perm = int(m[1], 16), int(m[2], 16), m[3]
    if perm == "r-xp":
        code_ranges.append((lo - IMAGE_BASE, hi - IMAGE_BASE))
    if perm[0] == "r":
        data_regions.append((lo, hi, perm))

def is_code_rva(a):
    ok = np.zeros(a.shape, dtype=bool)
    for lo, hi in code_ranges:
        ok |= (a >= lo) & (a < hi)
    return ok

print(f"pid {pid}: plain code RVAs {[(hex(l), hex(h)) for l, h in code_ranges]}, min run {MIN_RUN}, spread {SPREAD:#x}")
mem = open(f"/proc/{pid}/mem", "rb", buffering=0)
tables = []
for lo, hi, perm in data_regions:
    mem.seek(lo)
    try:
        buf = mem.read(hi - lo)
    except OSError:
        continue
    for align in range(4):                     # tables are 4-aligned, but be tolerant of region offset
        sub = buf[align:]
        n = len(sub) // 4
        if n < MIN_RUN:
            continue
        w = np.frombuffer(sub[:n * 4], dtype="<u4")
        good = is_code_rva(w)
        # runs of consecutive valid words
        edges = np.flatnonzero(np.diff(np.concatenate(([0], good.view(np.int8), [0]))))
        for s, e in zip(edges[::2], edges[1::2]):
            if e - s < MIN_RUN:
                continue
            run = w[s:e].astype(np.int64)
            # a real case table clusters: the bodies of one function. Drop random-looking runs.
            if run.max() - run.min() > SPREAD:
                continue
            # and most entries should differ (a table of one repeated default target is just padding)
            if len(set(run.tolist())) < max(4, (e - s) // 4):
                continue
            tables.append((lo + align + 4 * s, e - s, perm, run))
    # (later alignments only add duplicates of the same table in practice)
    break

seen, uniq = set(), []
for addr, n, perm, run in sorted(tables, key=lambda t: t[0]):
    if addr in seen:
        continue
    seen.add(addr)
    uniq.append((addr, n, perm, run))

print(f"\n{len(uniq)} candidate jump table(s) with >= {MIN_RUN} clustered code RVAs\n")
for addr, n, perm, run in sorted(uniq, key=lambda t: -t[1])[:40]:
    ents = [IMAGE_BASE + int(x) for x in run[:4]]
    print(f"  table @ {addr:#x}  {n:4d} entries ({perm})   first targets: " + ", ".join(f"{e:#x}" for e in ents))
