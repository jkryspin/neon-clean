#!/usr/bin/env python3
"""Does an unknown packet carry the position of the entity it refers to?

  poscorr.py <packet log> <opcode hex> <id offset> [player|mob|npc] [tolerance px, default 25]

Tracks entity positions from verified movement packets (PlayerMove 0x029d, MobMove 0x03dc, MobSpawn 0x03c9/0x03d5: the end
of the last 21-byte fragment, else the start). For every packet of the opcode it reads the entity id at <id offset>, looks up
that entity's latest position, and then asks, for every offset: is there an (x, y) pair of i16 (or i32) at (off, off+2)
(or (off, off+4)) within the tolerance of that position? Prints the offsets where it holds for most packets.
"""
import collections, re, struct, sys

log, op = sys.argv[1], sys.argv[2].lower().replace("0x", "").zfill(4)
idoff = int(sys.argv[3])
kind = sys.argv[4] if len(sys.argv) > 4 else "player"
tol = int(sys.argv[5]) if len(sys.argv) > 5 else 25

def end_pos(b, start, tail, count_at, base):
    n = b[count_at] if len(b) > count_at else 0
    if n and len(b) == start + tail + 21 * n:
        r = start + 21 * (n - 1)
        return struct.unpack_from("<hh", b, r + 2)
    return struct.unpack_from("<hh", b, base)

pos = {"player": {}, "mob": {}, "npc": {}}
hits = collections.defaultdict(int)
tested = 0
for line in open(log, errors="replace"):
    m = re.match(r"\S+ op (\w+) len \d+: ?(.*)", line)
    if not m or not m[2].strip():
        continue
    o, b = m[1], bytes.fromhex(m[2].replace(" ", ""))
    try:
        if o == "029d" and len(b) >= 14:
            pos["player"][struct.unpack_from("<I", b, 2)[0]] = end_pos(b, 19, 1, 18, 10)
        elif o == "03dc" and len(b) >= 34:
            pos["mob"][struct.unpack_from("<I", b, 2)[0]] = end_pos(b, 39, 2, 38, 30)
        elif o in ("03c9", "03d5") and len(b) >= 210:
            pos["mob"][struct.unpack_from("<I", b, 3)[0]] = struct.unpack_from("<hh", b, 206)
        elif o == "0456" and len(b) >= 26:
            pos["npc"][struct.unpack_from("<I", b, 2)[0]] = end_pos(b, 25, 1, 24, 16)
    except struct.error:
        pass
    if o != op or len(b) < idoff + 4:
        continue
    ent = pos[kind].get(struct.unpack_from("<I", b, idoff)[0])
    if ent is None:
        continue
    tested += 1
    ex, ey = ent
    for off in range(2, len(b) - 3):
        for width, fmt in ((2, "<hh"), (4, "<ii")):
            if off + 2 * width <= len(b):
                x, y = struct.unpack_from(fmt, b, off)
                if abs(x - ex) <= tol and abs(y - ey) <= tol:
                    hits[(off, width)] += 1
print(f"0x{op}: {tested} packets whose {kind} id (@{idoff}) has a known position")
for (off, w), h in sorted(hits.items(), key=lambda kv: -kv[1])[:6]:
    print(f"  (x, y) as {'i16' if w == 2 else 'i32'} pair at @{off}: within {tol}px of the {kind}'s last position in {h}/{tested} ({h / tested:.0%})")
