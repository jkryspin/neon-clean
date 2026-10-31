#!/usr/bin/env python3
"""Which opcodes arrive at a regular interval (heartbeats, timers, periodic updates)?

  periodic.py <packet log> [min packets, default 20]

For each opcode, the gaps between consecutive packets: if the median gap is >= 1 s and at least 70% of the gaps lie within 15% of it, the
opcode is periodic. Prints the period, how regular it is, and the packet size. The log's time stamps are quantised (about a second for
incoming packets), so only periods of several seconds are meaningful.
"""
import collections, re, statistics, sys

log = sys.argv[1]
min_n = int(sys.argv[2]) if len(sys.argv) > 2 else 20
ts = collections.defaultdict(list)
size = collections.defaultdict(collections.Counter)
for line in open(log, errors="replace"):
    m = re.match(r"(\S+) op (\w+) len (\d+):", line)
    if m:
        ts[m[2]].append(float(m[1])); size[m[2]][int(m[3])] += 1
rows = []
for op, v in ts.items():
    if len(v) < min_n:
        continue
    v.sort()
    gaps = [b - a for a, b in zip(v, v[1:]) if b - a > 0.2]
    if len(gaps) < min_n // 2:
        continue
    med = statistics.median(gaps)
    if med < 1.0:
        continue
    ok = sum(1 for g in gaps if abs(g - med) <= 0.15 * med) / len(gaps)
    if ok >= 0.7:
        rows.append((med, ok, op, len(v), size[op].most_common(1)[0][0]))
for med, ok, op, n, sz in sorted(rows):
    print(f"0x{op}: every {med:.1f} s ({ok:.0%} of {n} gaps within 15%), {sz} bytes")
if not rows:
    print("no periodic opcode")
