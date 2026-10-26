#!/usr/bin/env python3
"""Pointer-chain search (read-only): from the addresses of live position fields up to a static root in the exe image.

  ptrchain.py <pid> <addresses.json> [levels] [out.json]

A chain is [root, k_n, ..., k_1]: read the 8-byte pointer at `root`, add k_n, read the pointer there, ...; the last
sum is the address of the X coordinate (Y follows at +4). Prints and saves every chain found.
"""
import time
import json, os, sys
import numpy as np

pid, targets = int(sys.argv[1]), sorted(set(json.load(open(sys.argv[2]))))
levels = int(sys.argv[3]) if len(sys.argv) > 3 else 3
out = sys.argv[4] if len(sys.argv) > 4 else None
SLACK = 0x300
IMG = (0x140000000, 0x146000000)
os.nice(10)
mem = os.open(f'/proc/{pid}/mem', os.O_RDONLY)
blobs = []
for line in open(f'/proc/{pid}/maps'):
    p = line.split()
    if not p[1].startswith('rw') or 'dri' in line or 'nvidia' in line: continue
    a, b = [int(x, 16) for x in p[0].split('-')]
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

def parents(cur):
    cur = np.array(sorted(cur), dtype=np.uint64)
    found = []
    for base, arr in blobs:
        i = np.searchsorted(cur, arr)
        j = np.nonzero(i < len(cur))[0]
        t = cur[i[j]]
        hit = j[(arr[j] <= t) & (t - arr[j] < SLACK) & (arr[j] > 0x1000000)]
        for k in hit: found.append((base + int(k) * 8, int(arr[k]), int(cur[i[k]])))
    return found

levs = []
cur = targets
for lv in range(levels):
    f = parents(cur)
    levs.append(f)
    print('level', lv + 1, len(f), flush=True)
    cur = sorted({w for w, _, _ in f if not (IMG[0] <= w < IMG[1])})
    if not cur or len(f) > 400000: break

# rebuild chains: a level-L word whose address is static starts a chain; its target is a level-(L-1) word address
chains = []
for L, f in enumerate(levs):
    by_word = [{w: (v, t) for w, v, t in lv} for lv in levs]
    for w, v, t in f:
        if not (IMG[0] <= w < IMG[1]): continue
        offs = [t - v]
        addr = t
        ok = True
        for l in range(L - 1, -1, -1):
            e = by_word[l].get(addr)
            if e is None: ok = False; break
            offs.append(e[1] - e[0]); addr = e[1]
        if ok: chains.append([w] + offs)
print(len(chains), 'chains')
if out: json.dump(chains, open(out, 'w'))
for c in chains[:80]: print(hex(c[0]), [hex(k) for k in c[1:]])
