#!/usr/bin/env python3
"""Per-climb statistics from a neon-loader --trace-file of a goto loop (descend, climb, ...).

  navstats.py <trace> <top-y> [bottom-y]

A climb is a stretch where the decision target is the top; it counts a fall whenever the character drops 50+ px
from a platform it had reached during that climb (apart from the descent legs), and the jump count.
"""
import re, sys

rows = []
for l in open(sys.argv[1]):
    m = re.match(r'(\d+) pos=(-?\d+),(-?\d+) gr=Some\((\w+)\) tgt=\S+ mobs=\d+ \| (.*?) \| (.*)', l)
    if m:
        t, x, y, g, dec, act = m.groups()
        rows.append((int(t), int(x), int(y), g == 'true', dec, act))
top = int(sys.argv[2])
legs = []
cur = None
for r in rows:
    to_top = ('(1305,%d)' % top) in r[4]
    to_bottom = 'walk to (399,155)' in r[4]
    kind = 'up' if to_top else 'down' if to_bottom else None
    if kind is None:
        continue
    if cur is None or cur['kind'] != kind:
        cur = {'kind': kind, 'rows': []}
        legs.append(cur)
    cur['rows'].append(r)
for i, leg in enumerate(legs):
    rs = leg['rows']
    dur = (rs[-1][0] - rs[0][0]) / 1000
    taps = sum(1 for r in rs if 'Tap("alt")' in r[5])
    best = rs[0][2]
    falls = []
    for k, r in enumerate(rs):
        if r[2] < best: best = r[2]
        if leg['kind'] == 'up' and r[2] - best >= 50 and r[3]:
            falls.append((round((r[0] - rs[0][0]) / 1000, 1), r[1], best, r[2]))
            best = r[2]
    print('leg %d %-4s %5.1fs  start y %5d  end y %5d  jumps %2d  falls %d %s' % (i + 1, leg['kind'], dur, rs[0][2], rs[-1][2], taps, len(falls), falls))
