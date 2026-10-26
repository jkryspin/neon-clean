#!/usr/bin/env python3
"""Measure the character's movement physics live (read-only memory polling + xdotool keys).

  jumpmeasure.py <pid> <window-id>

Stand on a long flat platform with room on both sides. Records x/y every ~10 ms (the fixed position chain) while it
walks, jumps straight up and jumps while holding a direction, and prints speed, jump height, hang time and reach.
"""
import os, struct, subprocess, sys, time

pid, win = int(sys.argv[1]), sys.argv[2]
mem = os.open(f'/proc/{pid}/mem', os.O_RDONLY)
ADDR = 0x109928


def rd():
    x, y = struct.unpack('<ii', os.pread(mem, 8, ADDR))
    g = struct.unpack('<I', os.pread(mem, 4, 0x109830 - 0x3e4))[0]
    return x, y, g


def key(kind, k): subprocess.run(['xdotool', kind, k])


def trace(secs, down=(), tap_at=None, tap='alt'):
    out, t0, tapped = [], time.time(), False
    for k in down: key('keydown', k)
    while True:
        t = time.time() - t0
        if t >= secs: break
        if tap_at is not None and not tapped and t >= tap_at:
            key('key', tap); tapped = True
        x, y, _ = rd()
        if abs(x) < 30000 and abs(y) < 30000 and (x, y) != (0, 0): out.append((t, x, y))
        time.sleep(0.008)
    for k in down: key('keyup', k)
    return out


def settle(): time.sleep(1.2)


subprocess.run(['xdotool', 'mousemove', '--window', win, '700', '125', 'click', '1']); time.sleep(0.4)
x0, y0, _ = rd()
print('start', x0, y0)

# walking speed
tr = trace(0.8, ['Right']); settle()
good = [p for p in tr if p[0] > 0.2]
if len(good) > 3:
    print('walk right: %.0f px/s' % ((good[-1][1] - good[0][1]) / (good[-1][0] - good[0][0])))
tr = trace(0.8, ['Left']); settle()
good = [p for p in tr if p[0] > 0.2]
if len(good) > 3:
    print('walk left : %.0f px/s' % ((good[0][1] - good[-1][1]) / (good[-1][0] - good[0][0])))

def jump_stats(label, tr, ground_y):
    air = [p for p in tr if p[2] < ground_y - 2]
    if not air:
        print(label, 'no jump seen'); return
    top = min(p[2] for p in tr)
    t_take = air[0][0]; t_land = max(p[0] for p in air)
    xs = [p[1] for p in tr if p[0] >= t_take - 0.05 and p[0] <= t_land + 0.05]
    print('%s: rise %d px, peak after %.2fs, airborne %.2fs, x moved %d px (%.0f -> %.0f)' % (label, ground_y - top, min(tr, key=lambda p: p[2])[0] - t_take, t_land - t_take, xs[-1] - xs[0], xs[0], xs[-1]))

for i in range(3):
    gy = rd()[1]
    jump_stats('standing jump %d' % (i + 1), trace(1.4, [], tap_at=0.2), gy); settle()
for d, opp in (('Right', 'Left'), ('Left', 'Right')):
    for i in range(2):
        gy = rd()[1]
        jump_stats('running jump %s %d' % (d, i + 1), trace(1.4, [d], tap_at=0.25), gy); settle()
        trace(0.5, [opp]); settle()
print('end', rd())
