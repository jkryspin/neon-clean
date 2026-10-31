#!/usr/bin/env python3
"""Find every adjacent (x, y) i32 pair in writable memory that lies exactly on a foothold of the current map.
With the character standing still the true position shows up in several copies (read-only).

  posfind.py <pid> <map-json> [out.json]
"""
import json, os, sys
from collections import Counter
import numpy as np

pid, mapjson = int(sys.argv[1]), sys.argv[2]
out = sys.argv[3] if len(sys.argv) > 3 else None
os.nice(10)
g = json.load(open(mapjson))
flat = [f for f in g['footholds'] if f['y1'] == f['y2']]
ys = np.array(sorted({f['y1'] for f in flat}), dtype=np.int32)
spans = {}
for f in flat: spans.setdefault(f['y1'], []).append((min(f['x1'], f['x2']), max(f['x1'], f['x2'])))
mem = os.open(f'/proc/{pid}/mem', os.O_RDONLY)
hits = []
for line in open(f'/proc/{pid}/maps'):
    p = line.split()
    if not p[1].startswith('rw') or 'dri' in line or 'nvidia' in line: continue
    a, b = [int(x, 16) for x in p[0].split('-')]
    try: d = os.pread(mem, b - a, a)
    except OSError: continue
    arr = np.frombuffer(d[:len(d) // 4 * 4], dtype='<i4')
    if len(arr) < 3: continue
    cand = np.nonzero(np.isin(arr[1:], ys) & (arr[:-1] > -3000) & (arr[:-1] < 4000))[0]
    for i in cand:
        x, y = int(arr[i]), int(arr[i + 1])
        if any(lo <= x <= hi for lo, hi in spans[y]): hits.append((a + int(i) * 4, x, y))
c = Counter((x, y) for _, x, y in hits)
print(len(hits), 'pairs; most common:', c.most_common(8))
if out: json.dump(hits, open(out, 'w'))
