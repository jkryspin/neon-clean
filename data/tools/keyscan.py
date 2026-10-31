#!/usr/bin/env python3
"""Find AES key schedules in the game's own (readable) memory and test them on captured traffic.

Read-only: chunked preads of /proc/<pid>/mem, run niced. For every 4-byte-aligned window that
looks like the start of an AES-128 / AES-256 expanded key (in memory byte order, or with each
32-bit word byte-reversed as OpenSSL-style tables store them) the full schedule is verified, and
the candidate key is tried on the first client->server packet of a capture that includes the
connection start (handshake IVs on the wire, packet encrypted with AES-OFB, no IV shuffle yet).

  keyscan.py <pid> <capture.pcap> <char_id_hex>
"""
import os, struct, sys
import numpy as np
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes

CHUNK = 8 << 20


def make_sbox():
    sbox = [0] * 256
    p = q = 1
    while True:
        p = p ^ ((p << 1) & 0xFF) ^ (0x1B if p & 0x80 else 0)
        q ^= (q << 1) & 0xFF
        q ^= (q << 2) & 0xFF
        q ^= (q << 4) & 0xFF
        if q & 0x80:
            q ^= 0x09
        rol = lambda v, n: ((v << n) | (v >> (8 - n))) & 0xFF
        x = q ^ rol(q, 1) ^ rol(q, 2) ^ rol(q, 3) ^ rol(q, 4)
        sbox[p] = x ^ 0x63
        if p == 1:
            break
    sbox[0] = 0x63
    return np.array(sbox, dtype=np.uint8)


SBOX = make_sbox()
assert SBOX[0x53] == 0xED
RCON = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1B, 0x36]


def expand(key: bytes) -> bytes:
    nk = len(key) // 4
    rounds = nk + 6
    w = [list(key[4 * i:4 * i + 4]) for i in range(nk)]
    for i in range(nk, 4 * (rounds + 1)):
        t = list(w[i - 1])
        if i % nk == 0:
            t = [int(SBOX[b]) for b in t[1:] + t[:1]]
            t[0] ^= RCON[i // nk - 1]
        elif nk > 6 and i % nk == 4:
            t = [int(SBOX[b]) for b in t]
        w.append([a ^ b for a, b in zip(w[i - nk], t)])
    return bytes(b for word in w for b in word)


def candidates(chunk: np.ndarray, base: int, reverse_words: bool):
    """Yield (address, key_bytes) for windows whose first schedule step checks out."""
    n = (len(chunk) // 4) * 4
    W = chunk[:n].reshape(-1, 4)
    if reverse_words:
        W = W[:, ::-1]
    if len(W) < 70:
        return
    R = np.roll(SBOX[W], -1, axis=1)  # SubWord(RotWord(w))
    for nk, step in ((4, 4), (8, 8)):
        rc = np.array([1, 0, 0, 0], dtype=np.uint8)
        lhs = W[nk:]
        rhs = W[:-nk] ^ R[nk - 1:-1] ^ rc
        m = np.all(lhs == rhs, axis=1)
        # Second word of the new round key must follow too.
        m2 = np.all(W[nk + 1:] == (W[1:-nk] ^ W[nk:-1]), axis=1)
        k = min(len(m), len(m2))
        for idx in np.nonzero(m[:k] & m2[:k])[0]:
            if idx + 4 * (nk + 6 + 1) > len(W):
                continue
            words = W[idx:idx + nk]
            yield base + 4 * int(idx), bytes(words.reshape(-1)), nk * 4


def full_check(chunk_bytes: bytes, off: int, key: bytes, reverse_words: bool) -> bool:
    exp = expand(key)
    raw = chunk_bytes[off:off + len(exp)]
    if len(raw) < len(exp):
        return False
    if reverse_words:
        raw = b''.join(raw[i:i + 4][::-1] for i in range(0, len(raw), 4))
    return raw == exp


def load_first_client_packet(pcap_path):
    sys.path.insert(0, os.path.dirname(__file__))
    import importlib.util
    spec = importlib.util.spec_from_file_location('mc', os.path.join(os.path.dirname(__file__), 'mcrypt_check.py'))
    src = open(spec.origin).read().replace('\nmain()\n', '\n')
    ns = {}
    exec(compile(src, 'mc', 'exec'), ns)
    flows = ns['load_pcap'](pcap_path)
    pick = None
    for (sp, dp), v in flows.items():
        if 8500 <= sp <= 8700 and len(v) > 2 and struct.unpack_from('<H', v, 0)[0] == 0x13:
            pick = (sp, dp)
    s2c = flows[pick]; c2s = flows[(pick[1], pick[0])]
    hs = s2c[2:2 + struct.unpack_from('<H', s2c, 0)[0]]
    ivs = {'iv_a': hs[8:12], 'iv_b': hs[12:16]}
    h = c2s[:4]
    n = ((h[0] << 8 | h[1]) ^ (h[2] << 8 | h[3]))
    n = ((n >> 8) | ((n & 255) << 8)) & 0xFFFF
    return ivs, c2s[4:4 + n]


def try_key(key: bytes, ivs, ct: bytes, char_id: bytes):
    """AES-OFB (classic MapleStory: IV repeated to 16 bytes, ECB-encrypt chained) on the first packet."""
    hits = []
    for name, iv in ivs.items():
        for mode_name, blockiv in (('x4', iv * 4),):
            ks = b''; cur = blockiv
            enc = Cipher(algorithms.AES(key), modes.ECB()).encryptor()
            while len(ks) < len(ct):
                cur = enc.update(cur); ks += cur
            pt = bytes(a ^ b for a, b in zip(ct, ks))
            if char_id in pt:
                hits.append((name, mode_name, pt))
    return hits


def main():
    pid, pcap, cid = int(sys.argv[1]), sys.argv[2], bytes.fromhex(sys.argv[3])
    ivs, ct = load_first_client_packet(pcap)
    print('IVs', {k: v.hex() for k, v in ivs.items()}, 'first packet', len(ct), 'bytes', flush=True)
    os.nice(19)
    mem = os.open(f'/proc/{pid}/mem', os.O_RDONLY)
    seen = set(); found = 0; scanned = 0
    for line in open(f'/proc/{pid}/maps'):
        parts = line.split()
        if not parts[1].startswith('rw-p') or 'dri' in line or 'nvidia' in line:
            continue
        a, b = [int(x, 16) for x in parts[0].split('-')]
        pos = a
        while pos < b:
            ln = min(CHUNK, b - pos)
            try:
                data = os.pread(mem, ln + 256, pos)  # overlap so windows spanning chunks are seen
            except OSError:
                break
            if not data:
                break
            arr = np.frombuffer(data, dtype=np.uint8)
            scanned += len(data)
            for rev in (False, True):
                for addr, key, klen in candidates(arr, pos, rev):
                    off = addr - pos
                    if not full_check(data, off, key, rev) or (addr, key) in seen:
                        continue
                    seen.add((addr, key)); found += 1
                    res = try_key(key, ivs, ct, cid)
                    print(f'schedule @ {addr:#x} AES-{klen * 8} {"rev" if rev else "std"} key={key.hex()}', 'MATCH' if res else '', flush=True)
                    for name, mode, pt in res:
                        print('  decrypts first packet with', name, mode, pt[:32].hex(), flush=True)
            pos += ln
    print(f'scanned {scanned >> 20} MB, {found} valid AES schedules')


if __name__ == '__main__':
    main()
