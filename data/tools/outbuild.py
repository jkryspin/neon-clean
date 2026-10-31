#!/usr/bin/env python3
"""Turn an outraw.py capture (raw packet objects + send lengths) into a session file (.nsr) of outgoing records.

  outbuild.py raw.jsonl out.nsr [--relative]

Times are absolute unix milliseconds by default, so the file merges with a stamped incoming log of the same period
(`neon-view --log in.log --log out.nsr`); --relative starts at 0 (for self-contained fixtures).

The packet object (rcx of the client's send-packet function 0x141660360, Classic World v1.0.1) holds the plaintext at +8 and
its length as a u32 at +0x428 (validated: equal to the wire length minus 4 for 21 of 21 packets, 2 ms apart). Each object is
paired with the first send after it, only to double-check that length. The output contains plaintext payloads, which can
carry login tokens: keep it local (it is git-ignored via run/ and *.nsr).
"""
import json, struct, sys

ev = sorted((json.loads(l) for l in open(sys.argv[1]) if l.strip()), key=lambda e: e["t"])
relative = "--relative" in sys.argv
t0 = ev[0]["t"] if (ev and relative) else 0
out, checked, mismatch = [], 0, 0
for i, e in enumerate(ev):
    if e["k"] != "pkt":
        continue
    o = bytes.fromhex(e["obj"])
    n = struct.unpack_from("<I", o, 0x428)[0]
    if not 2 <= n <= 0x400 - 8:
        continue  # not a packet we can trust
    nxt = next((x for x in ev[i + 1:] if x["k"] == "send"), None)
    if nxt and nxt["t"] - e["t"] < 0.2:
        checked += 1
        mismatch += nxt["wire"] - 4 != n
    out.append(json.dumps({"t": int((e["t"] - t0) * 1000), "d": "out", "hex": o[8:8 + n].hex()}))
open(sys.argv[2], "w").write("\n".join(out) + "\n")
print(f"{len(out)} outgoing records; length field agrees with the wire length for {checked - mismatch}/{checked} paired sends")
