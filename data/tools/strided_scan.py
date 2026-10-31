#!/usr/bin/env python3
"""Look for classic-MapleStory-style AES keys (8 significant bytes, one per 4-byte word) in readable
process memory and in a file, and test each on the first client packet. Read-only, niced.
  strided_scan.py <pid> <MapleStory.exe> <capture.pcap> <char_id_hex>"""
import os, sys, importlib.util
import numpy as np
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
ks = importlib.util.module_from_spec(importlib.util.spec_from_file_location('ks', os.path.join(os.path.dirname(os.path.abspath(__file__)), 'keyscan.py')))
ks.__spec__.loader.exec_module(ks)

def find_strided(arr, base):
    """Offsets where 32 bytes have zeros in positions i%4!=0 and 8 mostly-distinct nonzero key bytes."""
    n = len(arr) - 32
    if n <= 0: return
    # zero mask for 3 of every 4 bytes, evaluated at every byte offset via shifted views
    z = (arr == 0)
    nz = ~z
    ok = np.ones(n, dtype=bool)
    for i in range(32):
        v = z[i:i + n] if i % 4 else nz[i:i + n]
        ok &= v
        if not ok.any(): return
    for off in np.nonzero(ok)[0]:
        k = arr[off:off + 32:4]
        if len(set(k.tolist())) >= 6:
            yield base + int(off), bytes(arr[off:off + 32])

def main():
    pid, exe, pcap, cid = int(sys.argv[1]), sys.argv[2], sys.argv[3], bytes.fromhex(sys.argv[4])
    ivs, ct = ks.load_first_client_packet(pcap)
    os.nice(19)
    cands = {}
    d = np.frombuffer(open(exe, 'rb').read(), dtype=np.uint8)
    for off, key in find_strided(d, 0): cands.setdefault(key, f'exe+{off:#x}')
    mem = os.open(f'/proc/{pid}/mem', os.O_RDONLY)
    for line in open(f'/proc/{pid}/maps'):
        p = line.split()
        if not p[1].startswith('r') or 'dri' in line or 'nvidia' in line: continue
        a, b = [int(x, 16) for x in p[0].split('-')]
        pos = a
        while pos < b:
            ln = min(8 << 20, b - pos)
            try: data = os.pread(mem, ln + 64, pos)
            except OSError: break
            if not data: break
            for off, key in find_strided(np.frombuffer(data, dtype=np.uint8), pos): cands.setdefault(key, f'mem {off:#x}')
            pos += ln
    print(len(cands), 'strided key candidates', flush=True)
    best = []
    for key, where in cands.items():
        for nm, iv in ivs.items():
            enc = Cipher(algorithms.AES(key), modes.ECB()).encryptor(); cur = iv * 4; ksm = b''
            while len(ksm) < len(ct): cur = enc.update(cur); ksm += cur
            pt = bytes(x ^ y for x, y in zip(ct, ksm))
            sc = pt.count(0) / len(pt) + (0.5 if pt[1] <= 4 else 0) + (5 if cid in pt else 0)
            best.append((sc, where, nm, key[::4].hex(), pt[:24].hex()))
    best.sort(reverse=True)
    for r in best[:5]: print(r)

main()
