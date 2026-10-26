#!/usr/bin/env python3
"""Per-leg report of a goto-loop trace: duration, jumps pressed, planned jumps, retries and real falls.

  navlegs.py <trace> <top-y>

A leg is a stretch with one goto target. In an "up" leg a real fall is a drop of 60+ px from a point reached earlier in
the leg that is not a planned maneuver (a planned drop is a jump/fall edge, which the trace labels as such)."""
import re, sys

rows = []
for l in open(sys.argv[1]):
    m = re.match(r'(\d+) pos=(-?\d+),(-?\d+) gr=Some\((\w+)\) tgt=\S+ mobs=\d+ \| (.*?) \| (.*)', l)
    if m:
        t, x, y, g, dec, act = m.groups()
        rows.append((int(t), int(x), int(y), g == 'true', dec, act))
top = sys.argv[2]
legs, cur = [], None
for r in rows:
    kind = 'up' if ('(1305,%s)' % top) in r[4] else 'down' if 'walk to (1000,275)' in r[4] or 'go to (1000,275)' in r[4] else None
    if kind is None:
        continue
    if cur is None or cur[0] != kind:
        cur = (kind, [])
        legs.append(cur)
    cur[1].append(r)
n_up = 0
for kind, rs in legs:
    dur = (rs[-1][0] - rs[0][0]) / 1000
    taps = sum(1 for r in rs if 'Tap("alt")' in r[5])
    falls = []
    if kind == 'up':
        last_tap = -10**9
        start = None
        for i, r in enumerate(rs):
            if 'Tap("alt")' in r[5]:
                last_tap = r[0]
            airborne = not r[3]
            if airborne and i > 0 and r[2] > rs[i - 1][2] and start is None and r[0] - last_tap > 900:
                start = (r[0], r[1], rs[i - 1][2])        # began falling with no jump behind it
            if start is not None and (r[3] or r[0] - start[0] > 2500):
                drop = r[2] - start[2]
                if drop >= 60 and r[0] - last_tap > 900:
                    falls.append((round((start[0] - rs[0][0]) / 1000, 1), start[1], start[2], r[2]))
                start = None
    print('%-4s leg  %5.1fs  start (%d,%d)  end (%d,%d)  jump presses %2d  falls %d %s' % (kind, dur, rs[0][1], rs[0][2], rs[-1][1], rs[-1][2], taps, len(falls), falls))
