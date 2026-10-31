#!/usr/bin/env python3
"""Check that a capture's outgoing stream follows the classic MapleStory IV schedule.

Reads the shuffle table from MapleStory.exe, the handshake IVs from a capture that includes the
connection start, and verifies header(n+1) against the IV shuffled after packet n. Usage:
  mcrypt_check.py MapleStory.exe capture.pcap
"""
import struct, sys

def rol32(v, n): return ((v << n) | (v >> (32 - n))) & 0xFFFFFFFF

def load_pcap(path, port_lo=8500, port_hi=8700):
    d = open(path, 'rb').read(); lt = struct.unpack('<I', d[20:24])[0]; hdr = 20 if lt == 276 else 16 if lt == 113 else 14
    off = 24; flows = {}
    while off + 16 <= len(d):
        ts, us, incl, orig = struct.unpack('<IIII', d[off:off + 16]); off += 16; p = d[off:off + incl]; off += incl
        ip = p[hdr:]
        if len(ip) < 20 or ip[0] >> 4 != 4 or ip[9] != 6: continue
        ihl = (ip[0] & 15) * 4; tcp = ip[ihl:]; sp, dp = struct.unpack('>HH', tcp[:4]); doff = (tcp[12] >> 4) * 4
        seq = struct.unpack('>I', tcp[4:8])[0]; payload = tcp[doff:]
        if payload: flows.setdefault((sp, dp), []).append((seq, payload))
    out = {}
    for k, segs in flows.items():
        segs.sort(); buf = b''; nxt = None
        for seq, p in segs:
            if nxt is None: nxt = seq
            if seq < nxt: p = p[nxt - seq:]; seq = nxt
            if seq == nxt and p: buf += p; nxt += len(p)
        out[k] = buf
    return out

def shuffle_iv(iv, table):
    """Classic MapleStory IV update (verified against live memory transitions)."""
    v = [0xF2, 0x53, 0x50, 0xC6]
    for inp in iv:
        e = v[1]; a = inp
        m = (table[e] - inp) & 255; v[0] = (v[0] + m) & 255
        m = v[2] ^ table[a]; e = (e - m) & 255; v[1] = e
        e = v[3]; m = e; e = (e - v[0]) & 255; m = table[m]; m = (m + inp) & 255; m ^= v[2]; v[2] = m
        e = (e + table[a]) & 255; v[3] = e
        x = rol32(v[0] | (v[1] << 8) | (v[2] << 16) | (v[3] << 24), 3)
        v = [x & 255, (x >> 8) & 255, (x >> 16) & 255, (x >> 24) & 255]
    return bytes(v)

def main():
    exe = open(sys.argv[1], 'rb').read()
    i = exe.find(bytes.fromhex('ec3f77a445d071bf')); table = exe[i:i + 256]
    flows = load_pcap(sys.argv[2])
    pick = None
    for (sp, dp), v in flows.items():
        if 8500 <= sp <= 8700 and len(v) > 2 and struct.unpack_from('<H', v, 0)[0] == 0x13:
            pick = (sp, dp)
    if not pick:
        sys.exit('no connection start (handshake) in this capture')
    s2c = flows[pick]; c2s = flows[(pick[1], pick[0])]
    hs = s2c[2:2 + struct.unpack_from('<H', s2c, 0)[0]]
    print('handshake', hs.hex())
    iv = hs[9:13]  # send IV
    print('send IV', iv.hex())
    seeds = []; lens = []; off = 0
    while off + 4 <= len(c2s):
        h = c2s[off:off + 4]; a = (h[0] << 8) | h[1]; c = (h[2] << 8) | h[3]
        n = a ^ c; n = ((n >> 8) | ((n & 255) << 8)) & 0xFFFF
        if n < 2 or off + 4 + n > len(c2s): break
        seeds.append(a); lens.append(n); off += 4 + n
    consts = set()
    for a in seeds:
        consts.add(a ^ ((iv[2] << 8) | iv[3])); iv = shuffle_iv(iv, table)
    print(len(seeds), 'client frames; distinct header constants:', [f'{c:04x}' for c in consts],
          'OK: IV schedule reproduces every header' if len(consts) == 1 else 'MISMATCH')

main()
