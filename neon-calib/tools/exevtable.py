#!/usr/bin/env python3
"""Dump the vtable of a live game object reached from a known static root, and find who calls its methods.

  neon-calib/tools/exevtable.py ROOT_ADDR [PID]            # e.g. 0x143bf8e78 (the stats object)
  neon-calib/tools/exevtable.py ROOT_ADDR --callers=N      # also find call sites of virtual method #N

Why this works when the dispatch is hidden (docs/decode-plan.md): the protector virtualises the opcode->handler
EDGE, but game classes' own methods are plain code. Reading `object = *(u64*)ROOT; vtable = *(u64*)object`
needs no RTTI linkage (which the protector relocated) and no debugger - /proc/PID/mem reads only. For the stats
object all 48/48 virtual methods are in readable code.

That turns a known game-state root (neon-stream already tracks several) into a handle on real handler code:
  - the vtable lists the class's methods;
  - virtual calls compile to `mov rax,[rcx]; call [rax+DISP]` where DISP = 8*slot, so call sites of a given
    method are findable by their displacement;
  - a method that writes a stat field is, by construction, on the path of whatever packet changed that stat.

IMPORTANT: use a client that is IN-GAME. A client sitting at login/char-select has a null root (jib01 did).
Reads are strictly passive - the same operation neon-stream does continuously.
"""
import re, struct, sys

args = [a for a in sys.argv[1:] if not a.startswith("--")]
root = int(args[0], 0)
pid = int(args[1]) if len(args) > 1 else None
callers = next((int(a.split("=")[1]) for a in sys.argv if a.startswith("--callers=")), None)
if pid is None:
    import subprocess
    best = None
    for line in subprocess.run(["bash", "-c", "pgrep MapleStory.exe"], capture_output=True, text=True).stdout.split():
        try:
            m = open(f"/proc/{line}/mem", "rb", buffering=0)
            m.seek(root)
            v = struct.unpack("<Q", m.read(8))[0]
        except Exception:
            continue
        if v > 0x10000:
            best = int(line)
            break
    if best is None:
        sys.exit("no client has that root populated - is one actually in-game?")
    pid = best

mem = open(f"/proc/{pid}/mem", "rb", buffering=0)
regions = []
for line in open(f"/proc/{pid}/maps"):
    m = re.match(r"([0-9a-f]+)-([0-9a-f]+) (\S{4})", line)
    if m:
        regions.append((int(m[1], 16), int(m[2], 16), m[3]))

def region(a):
    for lo, hi, p in regions:
        if lo <= a < hi:
            return p
    return "unmapped"

def q(a):
    mem.seek(a)
    return struct.unpack("<Q", mem.read(8))[0]

obj = q(root)
vt = q(obj)
print(f"pid {pid}: root {root:#x} -> object {obj:#x} ({region(obj)}) -> vtable {vt:#x} ({region(vt)})")
methods = []
for i in range(256):
    fn = q(vt + 8 * i)
    if not (0x140001000 <= fn < 0x146000000):
        break
    methods.append(fn)
print(f"{len(methods)} virtual methods, {sum(1 for f in methods if region(f)=='r-xp')} in readable code")
for i, f in enumerate(methods):
    print(f"  slot {i:3d}  disp {8*i:#06x}  {f:#x}  {'readable' if region(f)=='r-xp' else region(f)}")

if callers is not None:
    disp = 8 * callers
    print(f"\ncall sites of virtual slot {callers} (call [reg+{disp:#x}]) in the plain image:")
    lo, hi = 0x140001000, 0x14334b000
    mem.seek(lo)
    code = mem.read(hi - lo)
    # call/jmp qword ptr [reg+disp32]:  FF /2 or FF /4 with modrm mod=10 (disp32). Encodings: FF 90+r disp32 (call),
    # FF A0+r disp32 (jmp), optionally REX-prefixed (41). Match the disp32 bytes after the opcode+modrm.
    dpat = struct.pack("<I", disp) if disp >= 0x80 else None
    found = []
    if dpat:
        for modrm in list(range(0x90, 0x98)) + list(range(0xA0, 0xA8)):
            needle = bytes([0xFF, modrm]) + dpat
            s = 0
            while True:
                i = code.find(needle, s)
                if i < 0:
                    break
                found.append((lo + i, "call" if modrm < 0xA0 else "jmp"))
                s = i + 1
    found.sort()
    print(f"  {len(found)} site(s)")
    for a, kind in found[:60]:
        print(f"    {a:#x}  {kind}")
