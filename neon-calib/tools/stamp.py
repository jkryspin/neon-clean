import sys, time
# Prefixes each `op ...` line from `NEON_DUMP_OPS=all neon-stream` with a time.
# Newer streams print `ts <unix seconds> op ...` with the packet's real capture time (from tcpdump); that time is used when present.
# Older streams print `op ...` only: the time is then the moment the line is read (batched, about one second granularity).
f = open(sys.argv[1], 'a', buffering=1)
for l in sys.stdin:
    if l.startswith('ts '):
        parts = l.split(' ', 2)
        f.write('%s %s' % (parts[1], parts[2]))
    elif l.startswith('op '):
        f.write('%.3f %s' % (time.time(), l))
