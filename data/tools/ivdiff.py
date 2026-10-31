#!/usr/bin/env python3
"""Find the cipher IV in memory: a dword whose upper 16 bits XOR-change between snapshots exactly like
the outgoing packet header seeds do.  ivdiff.py <prefix> <capture.pcap> <n>"""
import json, struct, sys
import numpy as np

def outgoing_headers(path):
    d = open(path, 'rb').read(); lt = struct.unpack('<I', d[20:24])[0]; hdr = 20 if lt == 276 else 16 if lt == 113 else 14
    off = 24; flows = {}
    while off + 16 <= len(d):
        ts, us, incl, orig = struct.unpack('<IIII', d[off:off + 16]); off += 16; p = d[off:off + incl]; off += incl
        ip = p[hdr:]
        if len(ip) < 20 or ip[0] >> 4 != 4 or ip[9] != 6: continue
        ihl = (ip[0] & 15) * 4; tcp = ip[ihl:]; sp, dp = struct.unpack('>HH', tcp[:4]); doff = (tcp[12] >> 4) * 4
        seq = struct.unpack('>I', tcp[4:8])[0]; payload = tcp[doff:]
        if payload and 8500 <= dp <= 8700:
            flows.setdefault((sp, dp), []).append((ts + us / 1e6, seq, payload))
    out = []
    for k, segs in flows.items():
        segs.sort(key=lambda s: s[1]); buf = b''; times = []; nxt = None
        for ts, seq, p in segs:
            if nxt is None: nxt = seq
            if seq < nxt: p = p[nxt - seq:]; seq = nxt
            if seq == nxt and p:
                times.append((len(buf), ts)); buf += p; nxt += len(p)
        # frame it
        off = 0
        def tsat(o):
            t = times[0][1]
            for so, st in times:
                if so <= o: t = st
            return t
        while off + 4 <= len(buf):
            h = buf[off:off + 4]; a = (h[0] << 8) | h[1]; c = (h[2] << 8) | h[3]
            n = a ^ c; n = ((n >> 8) | ((n & 255) << 8)) & 0xFFFF
            if n < 2 or off + 4 + n > len(buf): break
            out.append((tsat(off), a, n, buf[off + 4:off + 4 + n])); off += 4 + n
    out.sort()
    return out

def main():
    import itertools
    prefix, pcap, n = sys.argv[1], sys.argv[2], int(sys.argv[3])
    frames = outgoing_headers(pcap)
    idx = [json.load(open(f'{prefix}.{i}.idx')) for i in range(n)]
    opts = []
    for i, ix in enumerate(idx):
        after = next(f for f in frames if f[0] > ix['resume'])
        prior = [f for f in frames if f[0] < ix['stop']]
        o = {'after': after[1]}
        if prior:
            o['before'] = prior[-1][1]
        opts.append(o)
        print(f'snapshot {i}:', {k: f'{v:04x}' for k, v in o.items()})
    combos = list(itertools.product(*[sorted(o) for o in opts]))
    bins = [open(f'{prefix}.{i}.bin', 'rb') for i in range(n)]
    maps = [{a: (ln, off) for a, ln, off in ix['regions']} for ix in idx]
    common = [a for a in maps[0] if all(a in m for m in maps[1:])]
    hits = []
    for a in common:
        ln = min(m[a][0] for m in maps)
        raws = []
        for i in range(n):
            bins[i].seek(maps[i][a][1]); raws.append(np.frombuffer(bins[i].read(ln), dtype=np.uint8))
        for par in (0, 1):
            m_ = (ln - par) // 2 * 2
            xs = [r[par:par + m_].view('<u2') for r in raws]
            ch = np.ones(len(xs[0]), dtype=bool)
            for i in range(n - 1):
                ch &= xs[i] != xs[i + 1]
            pos = np.nonzero(ch)[0]
            if not len(pos): continue
            sub = [x[pos] for x in xs]
            for endian in ('le', 'be'):
                vs = sub if endian == 'le' else [((x << 8) | (x >> 8)).astype(np.uint16) for x in sub]
                for combo in combos:
                    h = [opts[i][combo[i]] for i in range(n)]
                    ok = np.ones(len(pos), dtype=bool)
                    for i in range(n - 1):
                        ok &= (vs[i] ^ vs[i + 1]) == (h[i] ^ h[i + 1])
                    for k in np.nonzero(ok)[0]:
                        addr = a + par + 2 * int(pos[k])
                        hits.append((addr, endian, combo, [int(v[k]) for v in vs]))
    print(len(hits), 'candidate IV locations')
    for hit in hits[:25]:
        print(hex(hit[0]), hit[1], hit[2], [f'{v:04x}' for v in hit[3]])
    json.dump(hits, open(f'{prefix}.hits.json', 'w'))

main()
