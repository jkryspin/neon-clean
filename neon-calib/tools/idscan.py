#!/usr/bin/env python3
"""Which unknown opcodes carry ids of things we know? Scan a stamped packet log.

  idscan.py run/view.NAME.log [min hits, default 8] [min share, default 0.8]

Known ids come from verified packets: mobs (0x03c9/0x03d5 oid@3, 0x03dc oid@2), players (0x022d char id@6, 0x029d @2),
drops (0x0471 oid@4). For every other opcode and every byte offset, count how many packets have a u32 there that is a
known id. A field that matches at least `min share` of an opcode's packets (and at least `min hits`) is printed: the
packet *refers to* a mob / player / drop at that offset. This is structure, not meaning.
"""
import collections, re, struct, sys

log = sys.argv[1]
min_hits = int(sys.argv[2]) if len(sys.argv) > 2 else 8
min_share = float(sys.argv[3]) if len(sys.argv) > 3 else 0.8
pk = []
mobs, players, drops = set(), set(), set()
for line in open(log, errors="replace"):
    m = re.match(r"\S+ op (\w+) len (\d+): ?(.*)", line)
    if not m:
        continue
    b = bytes.fromhex(m[3].replace(" ", "")) if m[3].strip() else b""
    op = m[1]
    pk.append((op, b))
    u = lambda off: struct.unpack_from("<I", b, off)[0] if len(b) >= off + 4 else None
    if op in ("03c9", "03d5"):
        mobs.add(u(3))
    elif op == "03dc":
        mobs.add(u(2))
    elif op == "022d":
        players.add(u(6))
    elif op == "029d":
        players.add(u(2))
    elif op == "0471":
        drops.add(u(4))
for _s in (mobs, players, drops):
    _s.discard(None)
    _s.difference_update({i for i in _s if i < 100000})  # real object ids are large; zeros and small counters would match everything
print(f"known ids: {len(mobs)} mobs, {len(players)} players, {len(drops)} drops; {len(pk)} packets")
KNOWN = {"03c9", "03d5", "03dc", "022d", "029d", "0471", "007e", "00a7"}
by = collections.defaultdict(list)
for op, b in pk:
    by[op].append(b)
rows = []
for op, bs in by.items():
    if op in KNOWN or len(bs) < min_hits:
        continue
    L = collections.Counter(len(b) for b in bs).most_common(1)[0][0]
    same = [b for b in bs if len(b) == L]
    if len(same) < min_hits:
        continue
    for off in range(2, L - 3):
        vals = [struct.unpack_from("<I", b, off)[0] for b in same]
        if len(set(vals)) < 3:
            continue  # a constant field is not an id
        for name, ids in (("mob", mobs), ("player", players), ("drop", drops)):
            hit = sum(1 for v in vals if v in ids)
            if hit >= min_hits and hit / len(same) >= min_share:
                rows.append((op, L, off, name, hit, len(same), len(set(vals))))
rows.sort(key=lambda r: (-r[5], r[0], r[3], r[2]))
seen = set()
for op, L, off, name, hit, n, distinct in rows:
    if (op, name) in seen:
        continue  # the lowest offset per (opcode, kind); overlapping offsets are the same bytes
    seen.add((op, name))
    print(f"0x{op} len {L}: u32@{off} is a known {name} id in {hit}/{n} packets ({distinct} distinct values)")
if not rows:
    print("no field matches")
