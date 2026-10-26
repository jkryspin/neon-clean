#!/usr/bin/env python3
"""Derive the wire-opcode map of one connection from its own traffic.

  wiremap.py <incoming log> [<wire log>]        (default wire log: <incoming log>.wire; writes <wire log>.map)

Client->server packets use opcode numbers on the wire that differ from the internal ones the registry describes, and for game packets
the numbers differ on every connection. This finds the ones that can be told from the traffic alone:
  * keepalive (internal 0x02f0): the wire opcode of the 2-byte packets that answer the incoming 2-byte ping 0x062e (same count within
    10%, and most of them 0-300 ms after a ping);
  * movement (internal 0x00db): the wire opcode whose packets carry, at @7, the server's millisecond clock (0x001d @2), 90% within 5 s;
  * the opcodes that are the same on every connection (0x00c5, 0x009c), mapped to themselves;
  * attack (internal 0x00e1): a wire opcode whose packets have the length and bytes of the internal attack fixture (>= 85% of the bytes
    after the opcode equal).
Everything else stays unmapped (never guessed). The result is {"<wire hex>": "<internal hex>"}.
"""
import bisect, collections, json, os, re, struct, sys

LINE = re.compile(r"(\S+) op (\w+) len (\d+): ?(.*)")

def read(path):
    out = []
    for line in open(path, errors="replace"):
        m = LINE.match(line)
        if m:
            out.append((float(m[1]), int(m[2], 16), bytes.fromhex(m[4].replace(" ", ""))))
    return out

# Wire numbers that are the same on every connection seen so far (two accounts, 2026-10-26); their registry entries use the wire number as id.
FIXED = (0x00c5, 0x009c)

def derive(incoming, wire, attack_fixture=None):
    """incoming/wire: lists of (t, opcode, bytes). Returns {wire opcode: internal opcode}."""
    result = {op: op for op in FIXED if any(o == op for t, o, b in wire)}
    pings = sorted(t for t, op, b in incoming if op == 0x062e and len(b) == 2)
    if len(pings) >= 5:
        by = collections.defaultdict(list)
        for t, op, b in wire:
            if len(b) == 2:
                by[op].append(t)
        for op, ts in by.items():
            if abs(len(ts) - len(pings)) > 0.1 * len(pings):
                continue
            ts.sort()
            hit = 0
            for p in pings:
                k = bisect.bisect_left(ts, p)
                if k < len(ts) and 0 <= ts[k] - p <= 0.3:
                    hit += 1
            if hit >= 0.8 * len(pings):
                result[op] = 0x02f0
    # movement (internal 0x00db): its u32@7 is the millisecond clock the server also sends in 0x001d @2 (every 300 s);
    # a wire opcode whose packets all carry a clock within 5 s of that tick, extrapolated with the capture times, is the movement packet
    ticks = sorted((t, struct.unpack_from("<I", b, 2)[0]) for t, op, b in incoming if op == 0x001d and len(b) >= 6)
    if ticks:
        tt = [t for t, v in ticks]
        score = collections.defaultdict(lambda: [0, 0])
        for t, op, b in wire:
            if len(b) < 20:
                continue
            k = min(range(len(ticks)), key=lambda i: abs(tt[i] - t))
            expected = (ticks[k][1] + int((t - ticks[k][0]) * 1000)) & 0xFFFFFFFF
            got = struct.unpack_from("<I", b, 7)[0]
            score[op][1] += 1
            if abs(((got - expected + 2 ** 31) % 2 ** 32) - 2 ** 31) < 5000:
                score[op][0] += 1
        for op, (hit, n) in score.items():
            if n >= 5 and hit >= 0.9 * n and op not in result:
                result[op] = 0x00db
    if attack_fixture:
        for op, group in group_by_op(wire).items():
            if all(len(b) == len(attack_fixture) for b in group[:5]):
                sims = [sum(1 for i in range(2, len(b)) if b[i] == attack_fixture[i]) / (len(b) - 2) for b in group[:5]]
                if min(sims) >= 0.85:
                    result[op] = 0x00e1
    return result

def group_by_op(wire):
    g = collections.defaultdict(list)
    for t, op, b in wire:
        g[op].append(b)
    return g

if __name__ == "__main__":
    inc_path = sys.argv[1]
    wire_path = sys.argv[2] if len(sys.argv) > 2 else inc_path + ".wire"
    fx = None
    fx_path = os.path.join(os.path.dirname(__file__), "..", "..", "neon-protocol", "tests", "fixtures", "out_packets.json")
    if os.path.exists(fx_path):
        fx = bytes.fromhex(json.load(open(fx_path))["0xe1_attack"]["plain"])
    m = derive(read(inc_path), read(wire_path), fx)
    json.dump({f"{k:04x}": f"{v:04x}" for k, v in sorted(m.items())}, open(wire_path + ".map", "w"))
    print(wire_path + ".map:", {f"{k:04x}": f"{v:04x}" for k, v in m.items()})
