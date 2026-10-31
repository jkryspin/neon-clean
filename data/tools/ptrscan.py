#!/usr/bin/env python3
"""Walk pointers upward from some addresses toward a static root in the exe image (read-only).

  ptrscan.py <pid> <addresses.json> [levels]

Level 1 finds 8-byte words in writable memory that point within 0x800 below any target; level 2 repeats for
those words' addresses, and so on. A word whose own address lies in the exe image (0x140000000..) is a static root.
"""
import time
import json, os, sys
import numpy as np

pid, targets = int(sys.argv[1]), sorted(set(json.load(open(sys.argv[2]))))
levels = int(sys.argv[3]) if len(sys.argv) > 3 else 3
SLACK = 0x300
IMG = (0x140000000, 0x146000000)
os.nice(10)
mem = os.open(f'/proc/{pid}/mem', os.O_RDONLY)
regions = []
for line in open(f'/proc/{pid}/maps'):
    p = line.split()
    if not p[1].startswith('rw') or 'dri' in line or 'nvidia' in line: continue
    a, b = [int(x, 16) for x in p[0].split('-')]
    regions.append((a, b))
blobs = []
for a, b in regions:
    # Read in 4 MB pieces with pauses: one huge read of /proc/pid/mem stalls the client (a scan once got it
    # disconnected from the server).
    parts = []
    for c in range(a, b, 4 << 20):
        try: parts.append(os.pread(mem, min(4 << 20, b - c), c))
        except OSError: break
        time.sleep(0.003)
    d = b''.join(parts)
    if not d: continue
    blobs.append((a, np.frombuffer(d[:len(d) // 8 * 8], dtype='<u8')))

cur = np.array(targets, dtype=np.uint64)
chain = {}
for lv in range(1, levels + 1):
    found = []
    for base, arr in blobs:
        i = np.searchsorted(cur, arr)
        ok = i < len(cur)
        j = np.nonzero(ok)[0]
        gap = cur[i[j]] - arr[j]
        hit = j[(arr[j] <= cur[i[j]]) & (gap < SLACK) & (arr[j] > 0x1000000)]
        for k in hit:
            found.append((base + int(k) * 8, int(arr[k]), int(cur[i[k]])))
    print(f'level {lv}: {len(found)} parent words', flush=True)
    roots = [f for f in found if IMG[0] <= f[0] < IMG[1]]
    for f in roots[:40]:
        print('  STATIC root %#x -> %#x (target %#x, +%#x)' % (f[0], f[1], f[2], f[2] - f[1]))
    for f in found:
        chain.setdefault(f[2], []).append(f)
    if not found or len(found) > 200000: break
    cur = np.array(sorted(set(f[0] for f in found if not (IMG[0] <= f[0] < IMG[1]))), dtype=np.uint64)
    if len(cur) == 0: break
