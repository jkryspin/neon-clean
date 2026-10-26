#!/usr/bin/env python3
"""Find the camera's world X in client memory (read-only), using the player's X as a reference.

  camscan.py <pid> <window-id>

Stand in a safe, flat map with the fixed position chain reading correctly. Takes snapshots of every writable region
at three settled standing positions (walking left, then right, waiting for the camera to catch up each time) and keeps
the 32-bit words whose change equals the player's change: those are the camera (or copies of it). Prints candidates
as (address, value, value - player_x) with the word after each (the camera's Y, if it is stored next to X).
"""
import os, struct, subprocess, sys, time
import numpy as np

pid, win = int(sys.argv[1]), sys.argv[2]
MODE = sys.argv[3] if len(sys.argv) > 3 else 'int'
DT = {'int': '<i4', 'float': '<f4', 'double': '<f8'}[MODE]
W = 8 if MODE == 'double' else 4
mem = os.open(f'/proc/{pid}/mem', os.O_RDONLY)
os.nice(10)
regions = []
for line in open(f'/proc/{pid}/maps'):
    p = line.split()
    if not p[1].startswith('rw') or 'dri' in line or 'nvidia' in line: continue
    a, b = [int(x, 16) for x in p[0].split('-')]
    regions.append((a, b))


def player():
    return struct.unpack('<ii', os.pread(mem, 8, 0x109830 + 0xf8))


def snap():
    out = []
    for a, b in regions:
        try: d = os.pread(mem, b - a, a)
        except OSError: d = b''
        out.append(np.frombuffer(d[:len(d) // W * W], dtype=DT).copy())
    return out


def walk(key, secs):
    subprocess.run(['xdotool', 'keydown', key])
    time.sleep(secs)
    subprocess.run(['xdotool', 'keyup', key])
    time.sleep(2.2)  # let the camera settle


subprocess.run(['xdotool', 'mousemove', '--window', win, '700', '125', 'click', '1'])
time.sleep(0.4)
p0 = player(); s0 = snap()
walk('Left', 0.9); p1 = player(); s1 = snap()
walk('Right', 1.5); p2 = player(); s2 = snap()
print('player x:', p0[0], p1[0], p2[0], flush=True)
d1, d2 = p1[0] - p0[0], p2[0] - p1[0]
assert abs(d1) > 40 and abs(d2) > 40, 'the player did not move; is the window focused?'
tol = float(os.environ.get('CAM_TOL', 3 if MODE == 'int' else 2.5))
hits = []
for i, (a, b) in enumerate(regions):
    n = min(len(s0[i]), len(s1[i]), len(s2[i]))
    x0, x1, x2 = (s[i][:n].astype(np.float64) for s in (s0, s1, s2))
    fin = np.isfinite(x0) & np.isfinite(x1) & np.isfinite(x2)
    same = (np.abs(x1 - x0 - d1) <= tol) & (np.abs(x2 - x1 - d2) <= tol)
    flip = (np.abs(x1 - x0 + d1) <= tol) & (np.abs(x2 - x1 + d2) <= tol)
    ok = fin & (same | flip) & (np.abs(x2) < 20000) & (np.abs(x2 - p2[0]) > 3)
    for j in np.nonzero(ok)[0]:
        nxt = float(s2[i][j + 1]) if j + 1 < len(s2[i]) else None
        hits.append((a + int(j) * W, float(x2[j]), float(x2[j]) - p2[0], nxt))
print(len(hits), 'words move exactly like the player')
from collections import Counter
print(Counter(h[2] for h in hits).most_common(8))
for h in hits[:60]: print('%#x value %.1f (player%+.1f) next %s' % h)
