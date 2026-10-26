#!/usr/bin/env python3
"""Keep only the newest stdin line in a file: latest.py <path> [max writes per second, default 5].

Used by scripts/view.sh: neon-stream prints 50 JSON states per second; the view only needs the latest one.
The file is replaced atomically, so a reader never sees half a line.
"""
import os, sys, time
path = sys.argv[1]
period = 1.0 / float(sys.argv[2]) if len(sys.argv) > 2 else 0.2
tmp = path + ".tmp"
last_write = 0.0
pending = None
for line in sys.stdin:
    pending = line
    now = time.time()
    if now - last_write >= period:
        with open(tmp, "w") as f:
            f.write(pending)
        os.replace(tmp, path)
        last_write, pending = now, None
