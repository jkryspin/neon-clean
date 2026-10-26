#!/usr/bin/env python3
"""Re-find the player's X coordinate in client memory by differential scanning (read-only).

  posscan.py <pid> <window-id>

Takes snapshots of every writable region while the character walks right, right, then left with xdotool key
presses, and keeps the 32-bit values that moved consistently: S1-S0 and S2-S1 both positive and similar,
S3-S2 negative. Prints the surviving addresses with the value at +4 (Y), to compare with the on-screen position.
"""
import os, subprocess, sys, time
import numpy as np

pid, win = int(sys.argv[1]), sys.argv[2]
env = dict(os.environ)
os.nice(10)
mem = os.open(f'/proc/{pid}/mem', os.O_RDONLY)

regions = []
for line in open(f'/proc/{pid}/maps'):
    p = line.split()
    if not p[1].startswith('rw') or 'dri' in line or 'nvidia' in line: continue
    a, b = [int(x, 16) for x in p[0].split('-')]
    regions.append((a, b))


def snap():
    out = []
    for a, b in regions:
        try: data = os.pread(mem, b - a, a)
        except OSError: data = b''
        n = len(data) // 4 * 4
        out.append(np.frombuffer(data[:n], dtype='<i4').copy())
    return out


def walk(key, secs):
    subprocess.run(['xdotool', 'keydown', key], env=env)
    time.sleep(secs)
    subprocess.run(['xdotool', 'keyup', key], env=env)
    time.sleep(1.4)


subprocess.run(['xdotool', 'windowfocus', win], env=env)
subprocess.run(['xdotool', 'mousemove', '--window', win, '400', '300', 'click', '1'], env=env)  # the game only takes keys after a click inside it
time.sleep(0.5)
s0 = snap()
walk('Right', 0.7)
s1 = snap()
cands = []
for i in range(len(s1)):
    n = min(len(s0[i]), len(s1[i]))
    d = s1[i][:n].astype(np.int64) - s0[i][:n]
    idx = np.nonzero((d >= 25) & (d <= 300) & (np.abs(s0[i][:n]) < 20000))[0]
    if len(idx): cands.append((i, idx, d[idx]))
print('after step 1:', sum(len(c[1]) for c in cands), flush=True)
walk('Right', 0.7)
s2 = snap()
nxt = []
for i, idx, d1 in cands:
    idx = idx[idx < len(s2[i])]
    d1 = d1[:len(idx)]
    d2 = s2[i][idx].astype(np.int64) - s1[i][idx]
    ok = (d2 >= 25) & (d2 <= 300) & (np.abs(d2 - d1) <= 0.5 * np.abs(d1) + 15)
    if ok.any(): nxt.append((i, idx[ok], d2[ok]))
print('after step 2:', sum(len(c[1]) for c in nxt), flush=True)
walk('Left', 1.4)
s3 = snap()
final = []
for i, idx, d2 in nxt:
    idx = idx[idx < len(s3[i])]
    d3 = s3[i][idx].astype(np.int64) - s2[i][idx]
    ok = (d3 <= -40) & (d3 >= -600)
    for j in idx[ok]:
        final.append((regions[i][0] + int(j) * 4, int(s0[i][j]), int(s1[i][j]), int(s2[i][j]), int(s3[i][j]), int(s3[i][j + 1]) if j + 1 < len(s3[i]) else None))
print('after step 3:', len(final))
import json
json.dump([r[0] for r in final], open(os.environ.get('POSSCAN_OUT', '/dev/null'), 'w'))
for r in final[:60]:
    print('%#x x: %d -> %d -> %d -> %d  next i32: %s' % r)
