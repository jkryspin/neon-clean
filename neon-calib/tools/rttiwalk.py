#!/usr/bin/env python3
"""Recover a C++ class's vtable (and so its virtual methods) from the client's MSVC RTTI, statically.

  neon-calib/tools/rttiwalk.py CLASSNAME [PID]      e.g. rttiwalk.py CUserPool

The client keeps MSVC RTTI: 1745 `.?AV<name>@@` type-descriptor strings survive in the unpacked image,
including real MapleStory class names (CUserPool, CUser, CMob, CNpc, CMobZoneMan, ...). MSVC lays this out as:

    TypeDescriptor      : [vftable ptr][spare][".?AVCUserPool@@"]
    CompleteObjectLocator: {..., RVA(TypeDescriptor), RVA(ClassHierarchyDescriptor), ...}
    vtable              : [ -8 : ptr to CompleteObjectLocator ][ +0: method0 ][ +8: method1 ] ...

so walking name -> TD -> COL -> vtable -> methods gives a class's virtual functions without running, patching
or injecting anything. All cross-references in x64 RTTI are 32-bit image-relative (RVA), hence the image base.

Why this matters here: the receive dispatch is virtualised (docs/decode-plan.md), but the HANDLERS are in
readable code (16.2% of main-thread samples). RTTI is a way to reach named handler code directly, skipping the
obfuscated dispatch edge entirely.

Reads /proc/PID/mem by seek - no ptrace, no debugger, no writes.
"""
import re, struct, sys

IMAGE_BASE = 0x140000000

name = sys.argv[1] if len(sys.argv) > 1 else "CUserPool"
pid = int(sys.argv[2]) if len(sys.argv) > 2 else None
if pid is None:
    import subprocess
    for line in subprocess.run(["bash", "-c", "pgrep MapleStory.exe"], capture_output=True, text=True).stdout.split():
        if "jib01" in open(f"/proc/{line}/maps", errors="replace").read():
            pid = int(line)
            break
    if pid is None:
        sys.exit("no jib01 client found; pass a PID explicitly")

regions = []
for line in open(f"/proc/{pid}/maps"):
    m = re.match(r"([0-9a-f]+)-([0-9a-f]+) (\S{4})", line)
    if m and m[3][0] == "r" and 0x140000000 <= int(m[1], 16) < 0x146000000:
        regions.append((int(m[1], 16), int(m[2], 16), m[3]))

mem = open(f"/proc/{pid}/mem", "rb", buffering=0)
blocks = []
for lo, hi, perm in regions:
    mem.seek(lo)
    try:
        blocks.append((lo, mem.read(hi - lo), perm))
    except OSError:
        pass

def find_all(needle):
    for lo, buf, _ in blocks:
        s = 0
        while True:
            i = buf.find(needle, s)
            if i < 0:
                break
            yield lo + i
            s = i + 1

def read_at(addr, n):
    for lo, buf, _ in blocks:
        if lo <= addr < lo + len(buf) - n:
            return buf[addr - lo:addr - lo + n]
    return None

# 1. type descriptor: the name string is at TD+0x10 (vftable ptr, spare, then the name)
tds = [a - 0x10 for a in find_all(f".?AV{name}@@".encode())]
print(f"pid {pid}: '{name}' -> {len(tds)} type descriptor(s): {', '.join(hex(t) for t in tds)}")
if not tds:
    sys.exit("not found - try: rttiwalk.py <name> and check the exact spelling from the RTTI dump")

for td in tds:
    td_rva = td - IMAGE_BASE
    # 2. COLs reference the TD by RVA; the TD RVA sits at COL+0x0c in the x64 layout
    cols = [a - 0x0c for a in find_all(struct.pack("<I", td_rva))]
    cols = [c for c in cols if read_at(c, 4) == b"\x01\x00\x00\x00"]  # signature==1 means x64 COL
    print(f"\n  TD {td:#x} (rva {td_rva:#x}) -> {len(cols)} complete object locator(s)")
    for col in cols:
        # 3. the vtable's slot [-8] points at the COL
        vts = list(find_all(struct.pack("<Q", col)))
        for v in vts:
            vt = v + 8
            print(f"    COL {col:#x} -> vtable {vt:#x}")
            # 4. read virtual method pointers until something stops looking like code
            for i in range(40):
                p = read_at(vt + 8 * i, 8)
                if not p:
                    break
                fn = struct.unpack("<Q", p)[0]
                if not (0x140001000 <= fn < 0x146000000):
                    break
                region = next((pm for l, b, pm in blocks if l <= fn < l + len(b)), "?")
                tag = "readable" if region == "r-xp" else ("PROTECTOR rwxp" if region == "rwxp" else region)
                print(f"      vtbl[{i:2d}] = {fn:#x}   ({tag})")
