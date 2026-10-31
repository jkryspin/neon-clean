import sys, time
# Prefixes each `op ...` line from `NEON_DUMP_OPS=all neon-stream` with a time.
# Newer streams print `ts <unix seconds> op ...` with the packet's real capture time (from tcpdump); that time is used when present.
# Older streams print `op ...` only: the time is then the moment the line is read (batched, about one second granularity).
f = open(sys.argv[1], 'a', buffering=1)
# Client->server packets (decrypted from the wire, `ts <t> wire op ...`) go to a separate file: their opcodes are not the ones the client's internal packet object uses.
fw = open(sys.argv[1] + '.wire', 'a', buffering=1)
for l in sys.stdin:
    if l.startswith('ts ') and ' wire op ' in l:
        parts = l.split(' ', 2)
        fw.write('%s %s' % (parts[1], parts[2].replace('wire op', 'op', 1)))
    elif l.startswith('ts '):
        parts = l.split(' ', 2)
        f.write('%s %s' % (parts[1], parts[2]))
    elif l.startswith('op '):
        f.write('%.3f %s' % (time.time(), l))
