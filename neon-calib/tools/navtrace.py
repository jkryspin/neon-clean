#!/usr/bin/env python3
"""Summarize the jumps in a neon-loader --trace-file: take-off point, direction, apex and where each jump landed.

  navtrace.py <trace>
"""
import re, sys

rows = []
for l in open(sys.argv[1]):
    m = re.match(r'(\d+) pos=(-?\d+),(-?\d+) gr=Some\((\w+)\) tgt=\S+ mobs=\d+ \| (.*?) \| (.*)', l)
    if m:
        t, x, y, gr, dec, act = m.groups()
        rows.append((int(t), int(x), int(y), gr == 'true', dec, act))
i = 0
n = 0
while i < len(rows):
    t, x, y, gr, dec, act = rows[i]
    if 'Tap("alt")' in act and gr:
        d = re.search(r'Jump\(Some\((\w+)\)\)|Jump\(None\)', dec)
        direction = re.search(r'Some\((\w+)\)', dec).group(1) if d and 'Some' in d.group(0) else '-'
        j = i + 1
        top = y
        while j < len(rows) and rows[j][0] - t < 2500:
            if rows[j][2] < top: top = rows[j][2]
            if rows[j][3] and rows[j][0] - t > 250 and abs(rows[j][2] - rows[j - 1][2]) <= 1:
                break
            j += 1
        lt, lx, ly = rows[min(j, len(rows) - 1)][:3]
        n += 1
        print('#%d t=%.1fs jump %-5s from (%d,%d) apex y %d -> landed (%d,%d) after %.2fs: dx %+d dy %+d' % (n, (t - rows[0][0]) / 1000, direction, x, y, top, lx, ly, (lt - t) / 1000, lx - x, ly - y))
        i = j
    i += 1
