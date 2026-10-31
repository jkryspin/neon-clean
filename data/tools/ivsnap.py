#!/usr/bin/env python3
"""Snapshot a process's writable memory while it is stopped (SIGSTOP/SIGCONT), for IV-state diffing.

  ivsnap.py <pid> <outprefix> <n_snapshots> <wait_seconds_between>

Writes <outprefix>.<i>.bin (concatenated regions) and <outprefix>.<i>.idx (JSON: addr,len,off list and the
wall-clock times it was stopped/resumed). Run a tcpdump capture alongside: the first outgoing packet after each
resume carries the header seed that corresponds to the IV held in that snapshot. Read-only.
"""
import json, os, signal, sys, time

pid, prefix, n, wait = int(sys.argv[1]), sys.argv[2], int(sys.argv[3]), float(sys.argv[4])
os.nice(10)

def regions():
    out = []
    for line in open(f'/proc/{pid}/maps'):
        p = line.split()
        if not p[1].startswith('rw-p') or 'dri' in line or 'nvidia' in line:
            continue
        a, b = [int(x, 16) for x in p[0].split('-')]
        out.append((a, b - a))
    return out

for i in range(n):
    mem = os.open(f'/proc/{pid}/mem', os.O_RDONLY)
    idx = []
    t_stop = time.time()
    os.kill(pid, signal.SIGSTOP)
    try:
        with open(f'{prefix}.{i}.bin', 'wb') as f:
            for a, ln in regions():
                off = f.tell()
                got = 0
                while got < ln:
                    try:
                        chunk = os.pread(mem, min(8 << 20, ln - got), a + got)
                    except OSError:
                        break
                    if not chunk:
                        break
                    f.write(chunk); got += len(chunk)
                if got:
                    idx.append((a, got, off))
    finally:
        os.kill(pid, signal.SIGCONT)
    t_resume = time.time()
    json.dump({'stop': t_stop, 'resume': t_resume, 'regions': idx}, open(f'{prefix}.{i}.idx', 'w'))
    print(f'snapshot {i}: {sum(r[1] for r in idx) >> 20} MB, frozen {t_resume - t_stop:.1f}s, resumed at {t_resume:.3f}', flush=True)
    os.close(mem)
    time.sleep(wait)
