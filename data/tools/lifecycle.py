#!/usr/bin/env python3
"""Does an opcode mark the start or the end of an entity's presence?

  lifecycle.py <packet log> [window seconds, default 5]

For every opcode with a 4-byte field holding the id of a known player or mob (like idscan), count that entity's movement packets in
the window before and after each such packet, and the same at random times for the same entities. A big drop (after << before) looks like a
removal (PlayerLeave 0x0209: 1992 -> 180 against 1 and 5 at chance); a big rise (before << after) looks like an arrival or a start.
Evidence only: a person decides the name.
"""
import bisect, collections, random, re, struct, sys

log = sys.argv[1]
W = float(sys.argv[2]) if len(sys.argv) > 2 else 5.0
mv = collections.defaultdict(list)       # (kind, id) -> movement times
pk = collections.defaultdict(list)       # op -> [(t, bytes)]
mobs, players = set(), set()
for line in open(log, errors="replace"):
    m = re.match(r"(\S+) op (\w+) len (\d+): ?(.*)", line)
    if not m or not m[4].strip():
        continue
    t, op, b = float(m[1]), m[2], bytes.fromhex(m[4].replace(" ", ""))
    if op == "029d" and len(b) >= 6:
        i = struct.unpack_from("<I", b, 2)[0]; mv[("player", i)].append(t); players.add(i)
    elif op == "03dc" and len(b) >= 6:
        i = struct.unpack_from("<I", b, 2)[0]; mv[("mob", i)].append(t); mobs.add(i)
    elif op in ("03c9", "03d5") and len(b) >= 7:
        mobs.add(struct.unpack_from("<I", b, 3)[0])
    elif op == "022d" and len(b) >= 10:
        players.add(struct.unpack_from("<I", b, 6)[0])
    else:
        pk[op].append((t, b))
for v in mv.values():
    v.sort()
def cnt(ts, a, b):
    return bisect.bisect_right(ts, b) - bisect.bisect_left(ts, a)
lo = min(t for v in mv.values() for t in v); hi = max(t for v in mv.values() for t in v)
rng = random.Random(5)
rows = []
for op, items in pk.items():
    if op in ("029d", "03dc", "0456", "0282") or len(items) < 30:
        continue
    L = collections.Counter(len(b) for _, b in items).most_common(1)[0][0]
    items = [(t, b) for t, b in items if len(b) == L]
    for off in range(2, min(L - 3, 64)):
        for kind, ids in (("player", players), ("mob", mobs)):
            ref = [(t, struct.unpack_from("<I", b, off)[0]) for t, b in items]
            ref = [(t, i) for t, i in ref if i in ids]
            if len(ref) < 30 or len(ref) / len(items) < 0.6 or len({i for _, i in ref}) < 5:
                continue
            bef = sum(cnt(mv.get((kind, i), []), t - W, t - 1e-3) for t, i in ref)
            aft = sum(cnt(mv.get((kind, i), []), t + 1e-3, t + W) for t, i in ref)
            rb = ra = 0
            for t, i in ref:
                tt = rng.uniform(lo, hi); ts = mv.get((kind, i), [])
                rb += cnt(ts, tt - W, tt); ra += cnt(ts, tt, tt + W)
            rows.append((op, L, off, kind, len(ref), bef, aft, rb, ra))
            break
print(f"window {W:.0f} s; entity movement packets before / after each packet (and at random times):")
seen = set()
for op, L, off, kind, n, bef, aft, rb, ra in sorted(rows, key=lambda r: -abs(r[5] - r[6]) / max(1, r[5] + r[6])):
    if (op, kind) in seen:
        continue
    seen.add((op, kind))
    tag = "removal-like" if aft < 0.3 * bef and bef > 3 * max(rb, 1) else "arrival-like" if bef < 0.3 * aft and aft > 3 * max(ra, 1) else ""
    print(f"  0x{op} len {L} {kind}@{off}: {n} packets, before {bef} / after {aft}  (random: {rb} / {ra})  {tag}")
