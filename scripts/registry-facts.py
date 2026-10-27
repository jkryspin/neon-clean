#!/usr/bin/env python3
"""Add measured FACTS (no meanings) to every incoming opcode that is only observed/structure, from exact-decoded logs.

  scripts/registry-facts.py [--dry] LOG...

For each incoming opcode seen in the logs: packet count, number of logs, the commonest sizes, the typical gap and whether it is periodic
(>= 70% of gaps within 15% of the median), the byte ranges that never change (at the dominant size), and u32 fields that hold ids
of players/mobs seen in verified packets. The sentence starts with "FACTS (" and is replaced on re-runs, so the script is idempotent.
Only the logs written since neon-stream decrypts exactly (2026-10-26 18:38) are meaningful: older ones were decoded with a heuristic.
Opcodes missing from the registry are added as `observed`. Run scripts/test-headless.sh afterwards.
"""
import collections, re, struct, sys

dry = "--dry" in sys.argv
logs = [a for a in sys.argv[1:] if not a.startswith("--")]
pk = collections.defaultdict(list)      # op -> [(t, bytes)]
nlogs = collections.defaultdict(set)
for path in logs:
    skipped = 0
    for line in open(path, errors="replace"):
        m = re.match(r"(\S+) op (\w+) len (\d+): ?(.*)", line)
        if m and m[4].strip():
            skipped += 1
            if skipped <= 13:   # the first 12 frames of a capture are decoded with the old heuristic (the IV is not known yet)
                continue
            op = int(m[2], 16)
            pk[op].append((float(m[1]), bytes.fromhex(m[4].replace(" ", ""))))
            nlogs[op].add(path)
mobs, players = set(), set()
for t, b in pk.get(0x03dc, []):
    mobs.add(struct.unpack_from("<I", b, 2)[0])
for t, b in pk.get(0x029d, []):
    players.add(struct.unpack_from("<I", b, 2)[0])
for t, b in pk.get(0x03c9, []):
    if len(b) >= 7:
        mobs.add(struct.unpack_from("<I", b, 3)[0])
# ids of real objects are large; 0 and other tiny values are decode noise (a truncated/garbage packet) and would match every
# all-zero field, which several packets have (explorer.rs uses the same guard)
mobs = {v for v in mobs if v >= 100_000}
players = {v for v in players if v >= 100_000}

def ranges(offs):
    out, i = [], 0
    offs = sorted(offs)
    while i < len(offs):
        j = i
        while j + 1 < len(offs) and offs[j + 1] == offs[j] + 1:
            j += 1
        out.append((offs[i], offs[j]))
        i = j + 1
    return out

def best_ref(op):
    """The single strongest (kind, offset) id-reference for this opcode's dominant-length packets, if any is strong enough to
    promote an 'observed' entry to 'structure' with a field (the same 0.8-share threshold as the FACTS sentence)."""
    rows = pk[op]
    lens = collections.Counter(len(b) for t, b in rows)
    if not lens:
        return None
    dom = lens.most_common(1)[0][0]
    same = [b for t, b in rows if len(b) == dom]
    if len(same) < 8 or dom < 6:
        return None
    best = None
    for o in range(2, min(dom - 3, 120)):
        vals = [struct.unpack_from("<I", b, o)[0] for b in same]
        if len(set(vals)) < 3:
            continue
        for kind, known in (("player", players), ("mob", mobs)):
            h = sum(1 for v in vals if v in known)
            share = h / len(vals)
            if share >= 0.8 and h >= 5 and (best is None or h > best[2]):
                best = (kind, o, h, len(vals))
    return best

def facts(op):
    rows = pk[op]
    n = len(rows)
    lens = collections.Counter(len(b) for t, b in rows)
    s = f"FACTS (exact-decoded logs, 2026-10-26): {n} packets in {len(nlogs[op])} log(s); sizes " + ", ".join(f"{l} x{c}" for l, c in lens.most_common(4))
    ts = sorted(t for t, b in rows)
    gaps = [b - a for a, b in zip(ts, ts[1:]) if b - a > 0.05]
    if len(gaps) >= 6:
        gaps.sort()
        med = gaps[len(gaps) // 2]
        share = sum(1 for g in gaps if abs(g - med) <= 0.15 * med) / len(gaps)
        s += f"; median gap {med:.1f} s" + (f", periodic ({share:.0%} of gaps within 15%)" if share >= 0.7 and med >= 0.5 else "")
    dom = lens.most_common(1)[0][0]
    same = [b for t, b in rows if len(b) == dom]
    if len(same) >= 5 and dom >= 3:
        const = [o for o in range(2, min(dom, 200)) if len({b[o] for b in same}) == 1]
        r = ranges(const)
        if r:
            s += "; constant bytes " + ", ".join((f"@{a}" if a == b else f"@{a}-{b}") + "=" + " ".join(f"{x:02x}" for x in same[0][a:min(b, a + 5) + 1]) + ("..." if b > a + 5 else "") for a, b in r[:5])
        refs = []
        for o in range(2, min(dom - 3, 120)):
            vals = [struct.unpack_from("<I", b, o)[0] for b in same]
            if len(set(vals)) < 3:
                continue
            for kind, known in (("player", players), ("mob", mobs)):
                h = sum(1 for v in vals if v in known)
                if h >= max(5, int(0.8 * len(vals))):
                    refs.append(f"u32@{o} is a known {kind} id in {h}/{len(vals)}")
        if refs:
            s += "; " + "; ".join(refs[:3])
    return s + ". Meaning not established."

text = open("neon-protocol/opcodes.toml").read()
parts = re.split(r"(?=\[\[opcode\]\])", text)
seen_in = set()
out = []
changed = added = 0
for b in parts:
    m = re.search(r'^id = (0x[0-9a-f]+)\ndir = "in"', b, re.M)
    if m:
        op = int(m[1], 16)
        seen_in.add(op)
        st = re.search(r'^status = "(\w+)"', b, re.M)
        named = re.search(r"^name = ", b, re.M) is not None
        if op in pk and st and st[1] in ("observed", "structure") and not named and "FACTS_SKIP" not in b:
            f = facts(op)
            sm = re.search(r'^summary = "(.*)"\n', b, re.M)
            if sm:
                body = re.sub(r" ?FACTS \(.*$", "", sm[1])
                new = 'summary = "' + (body + " " if body else "") + f.replace('"', "'") + '"\n'
                b = b[: sm.start()] + new + b[sm.end():]
            else:
                stm = re.search(r'^status = "\w+"\n', b, re.M)
                b = b[: stm.end()] + 'summary = "' + f.replace('"', "'") + '"\n' + b[stm.end():]
            # an 'observed' entry with a strong, single id-reference and no field array yet: promote it, like a human would
            if st[1] == "observed" and "field = [" not in b:
                br = best_ref(op)
                if br:
                    kind, off, hits, total = br
                    fname = "mob_oid" if kind == "mob" else "char_id"
                    b = re.sub(r'^status = "observed"\n', 'status = "structure"\n', b, count=1, flags=re.M)
                    b = b.rstrip("\n") + f'\nfield = [{{ name = "{fname}", ty = "u32", off = {off} }}]\n\n'
            changed += 1
    out.append(b)
extra = ""
for op in sorted(pk):
    # A brand-new opcode needs more than a handful of bytes seen once: a framing/desync glitch can split a real packet into
    # tiny leftover "packets" that happen to parse as a plausible-looking opcode, all at the exact same instant (seen once,
    # 2026-10-26: five bare 2-byte "opcodes" all at one timestamp, sandwiched in otherwise normal traffic, never seen again).
    # Require real temporal spread - at least 3 packets spanning more than 1 second - to rule that out.
    rows = pk[op]
    if op not in seen_in and len(rows) >= 3:
        ts = sorted(t for t, b in rows)
        if ts[-1] - ts[0] > 1.0:
            lens = collections.Counter(len(b) for t, b in rows)
            lo, hi = min(lens), max(lens)
            extra += f'[[opcode]]\nid = {op:#06x}\ndir = "in"\nstatus = "observed"\nsummary = "{facts(op).replace(chr(34), chr(39))}"\nlen_min = {lo}\nlen_max = {hi}\n\n'
            added += 1
print(f"{changed} entries updated, {added} opcodes added")
if not dry:
    res = "".join(out).rstrip("\n") + "\n\n" + extra.rstrip("\n") + ("\n" if extra else "")
    open("neon-protocol/opcodes.toml", "w").write(res)
