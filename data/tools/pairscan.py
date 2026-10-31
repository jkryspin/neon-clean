#!/usr/bin/env python3
"""Find request/response relations between outgoing and incoming packets by shared values.

  pairscan.py <log> [<more logs>...]      (the incoming log and the outgoing .nsr of the same period; absolute times)

For every outgoing packet it looks at the incoming packets of the next 2 s and compares every u16/u32 (little endian) in the first
96 bytes of both. A pair (out opcode @off, in opcode @off) is reported when the values match in at least 60% of the outgoing packets
of that opcode that have a candidate, there are at least 3 distinct matching values, and shuffling the incoming values
breaks it. Limitation: values drawn from a small set (coordinates, stances) match by accident, so a pair with only 3-8 distinct
values (as the 0x00db / 0x029d lines of a first run showed) is noise until a second capture confirms it; ids and counters (many distinct
values) are what this finds well (0x032b -> 0x0472, 0x02fe -> 0x03e7). Small values (< 256) are skipped for u32 and for u16 they must differ in >= 3 distinct cases.
"""
import collections, json, random, re, struct, sys

recs = []
for path in sys.argv[1:]:
    text_dir = "out" if path.endswith(".wire") else "in"   # a .wire file holds client->server packets (wire opcodes)
    for line in open(path, errors="replace"):
        line = line.strip()
        if line.startswith("{"):
            try:
                j = json.loads(line)
            except ValueError:
                continue
            recs.append((j["t"] / 1000.0, j["d"], bytes.fromhex(j["hex"])))
        else:
            m = re.match(r"(\S+) op \w+ len \d+: ?(.*)", line)
            if m and m[2].strip():
                recs.append((float(m[1]), text_dir, bytes.fromhex(m[2].replace(" ", ""))))
recs.sort(key=lambda r: r[0])
ins = [r for r in recs if r[1] == "in"]
outs = [r for r in recs if r[1] == "out"]
import bisect
in_t = [r[0] for r in ins]
def windows(b):
    n = min(len(b), 96)
    for off in range(2, n - 1):
        yield off, 2, struct.unpack_from("<H", b, off)[0]
        if off + 4 <= n:
            yield off, 4, struct.unpack_from("<I", b, off)[0]
stats = collections.defaultdict(lambda: [0, 0, set()])   # (oop, ooff, w, iop, ioff) -> [hits, tried, values]
by_op = collections.Counter()
for t, _, ob in outs:
    oop = ob[0] | ob[1] << 8 if len(ob) > 1 else -1
    by_op[oop] += 1
    lo, hi = bisect.bisect_right(in_t, t), bisect.bisect_right(in_t, t + 2.0)
    cand = ins[lo:hi]
    if not cand:
        continue
    ovals = {(off, w): v for off, w, v in windows(ob)}
    seen = set()
    for _, _, ib in cand:
        iop = ib[0] | ib[1] << 8
        for ioff, iw, iv in windows(ib):
            for (ooff, ow), ov in ovals.items():
                if ow == iw and ov == iv and (ow == 4 and ov > 1000 or ow == 2 and ov > 255):
                    key = (oop, ooff, ow, iop, ioff)
                    if key not in seen:
                        seen.add(key)
                        stats[key][0] += 1
                        stats[key][2].add(ov)
rows = []
for key, (hits, _, vals) in stats.items():
    oop = key[0]
    if len(vals) >= 3 and hits >= 3 and hits / by_op[oop] >= 0.6:
        rows.append((hits / by_op[oop], hits, by_op[oop], key, len(vals)))
rows.sort(reverse=True)
print(f"{len(outs)} outgoing, {len(ins)} incoming packets")
done = set()
for share, hits, n, (oop, ooff, w, iop, ioff), nv in rows:
    if (oop, iop, ooff) in done:
        continue
    done.add((oop, iop, ooff))
    print(f"out 0x{oop:04x} u{w * 8}@{ooff}  ==  in 0x{iop:04x} @{ioff}: {hits}/{n} outgoing packets ({share:.0%}), {nv} distinct values")
if not rows:
    print("no pairs")
