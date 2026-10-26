#!/usr/bin/env python3
"""Find "header + count byte + n fixed-size records (+ trailer)" layouts by brute force.

  recfind.py <packet log (.nsr or stamped)> [min packets, default 30] [min share, default 0.97]

For every opcode with enough packets it tries every (count offset c, trailer t, stride s) and counts the packets for which
  len == c + 1 + t + s * n   and   plain[c] == n   (n >= 1)
A layout that holds for nearly all packets of an opcode, with at least two different n, is almost certainly real: the
count byte is a checksum that chance does not satisfy. This only finds the *shape*; field meanings come from the explorer.
"""
import collections, json, re, sys

log = sys.argv[1]
min_n = int(sys.argv[2]) if len(sys.argv) > 2 else 30
min_share = float(sys.argv[3]) if len(sys.argv) > 3 else 0.97
by = collections.defaultdict(list)
for line in open(log, errors="replace"):
    line = line.strip()
    if line.startswith("{"):
        try:
            j = json.loads(line)
        except ValueError:
            continue
        b, d = bytes.fromhex(j["hex"]), j["d"]
    else:
        m = re.match(r"\S+ op \w+ len \d+: ?(.*)", line)
        if not m or not m[1].strip():
            continue
        b, d = bytes.fromhex(m[1].replace(" ", "")), "in"
    if len(b) >= 4:
        by[(d, b[0] | b[1] << 8)].append(b)
rows = []
for (d, op), bs in by.items():
    if len(bs) < min_n:
        continue
    best = None
    for c in range(2, 80):
        for t in range(0, 6):
            for s in range(4, 64):
                ok = 0
                ns = set()
                for b in bs:
                    rest = len(b) - (c + 1 + t)
                    if rest > 0 and rest % s == 0 and c < len(b) and b[c] == rest // s:
                        ok += 1
                        ns.add(rest // s)
                share = ok / len(bs)
                if share >= min_share and len(ns) >= 2 and (best is None or (share, -s) > (best[0], -best[3])):
                    best = (share, c, t, s, sorted(ns))
    if best:
        rows.append((len(bs), d, op) + best)
rows.sort(reverse=True)
for n, d, op, share, c, t, s, ns in rows:
    print(f"{'in ' if d == 'in' else 'out'} 0x{op:04x}: {n} packets, {share:.1%} fit  count byte @{c}, records start @{c + 1}, stride {s}, trailer {t}, n seen {ns[:6]}")
if not rows:
    print("no opcode has such a layout")
