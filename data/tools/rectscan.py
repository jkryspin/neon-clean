#!/usr/bin/env python3
"""List every entity hitbox in client memory (read-only): the 12-word signature R T L B | W H W H | L T R B.

  rectscan.py <pid> [map.json]

Prints each rect's foot position ((L+R)/2, B), size and address; with a map file, marks rects that coincide with
one of the map's NPCs.
"""
import json, os, sys
import numpy as np

pid = int(sys.argv[1])
npcs = json.load(open(sys.argv[2]))['npcs'] if len(sys.argv) > 2 else []
os.nice(10)
mem = os.open(f'/proc/{pid}/mem', os.O_RDONLY)
out = []
for line in open(f'/proc/{pid}/maps'):
    p = line.split()
    if not p[1].startswith('rw') or 'dri' in line or 'nvidia' in line: continue
    a, b = [int(x, 16) for x in p[0].split('-')]
    try: d = os.pread(mem, b - a, a)
    except OSError: continue
    w = np.frombuffer(d[:len(d) // 4 * 4], dtype='<i4')
    n = len(w) - 12
    if n <= 0: continue
    R, T, L, B, WW, HH = (w[i:i + n] for i in range(6))
    ok = (R - L == WW) & (B - T == HH) & (WW >= 8) & (WW <= 400) & (HH >= 8) & (HH <= 400)
    for i, k in enumerate((6, 7, 8, 9, 10, 11)):
        ref = (WW, HH, L, T, R, B)[i]
        ok &= (w[k:k + n] == ref)
    ok &= (np.abs(L) < 20000) & (np.abs(R) < 20000) & (np.abs(T) < 20000) & (np.abs(B) < 20000)
    for j in np.nonzero(ok)[0]:
        out.append((a + int(j) * 4, int(L[j]), int(T[j]), int(R[j]), int(B[j])))
seen = {}
for ad, l, t, r, b in out:
    seen.setdefault((l, t, r, b), []).append(ad)
for (l, t, r, b), ads in sorted(seen.items(), key=lambda kv: kv[0][0]):
    foot = ((l + r) // 2, b)
    tag = ''
    for n in npcs:
        if n['x'] is not None and abs(n['x'] - foot[0]) <= 25 and abs(n['y'] - foot[1]) <= 25: tag = ' NPC %d' % n['id']
    print('foot %s size %dx%d copies %d%s' % (foot, r - l, b - t, len(ads), tag))
