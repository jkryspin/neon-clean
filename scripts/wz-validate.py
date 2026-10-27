#!/usr/bin/env python3
"""Identify what every id-shaped packet field actually holds, by testing observed values against the client's
own name tables.

  scripts/wz-validate.py LOG...            # sweep every u32 field in the registry
  scripts/wz-validate.py --op 0x02af LOG...

For each registered u32 field (top-level and record fields), collect the values actually observed in captures
and ask which of the client's WZ tables they resolve in - Mob, Skill, Consume (use items), Eqp, Etc, Ins, Cash,
Npc, Map. A field whose values resolve ~100% in exactly one table is that kind of id, and the client's own data
says so; no statistical argument required.

This is how 0x02b9's "item-id-range numbers" were caught as actually being JOB SKILL ids, and how 0x02af's
u32@130 was confirmed to be a real mob template. Run it whenever a new id-shaped field appears.

Object ids (mob oid, char id, drop oid) are runtime handles, not content ids - they are expected to resolve in
NOTHING, and that is itself a useful signal (it distinguishes a handle from a content reference).
"""
import collections, json, os, re, struct, subprocess, sys

HEURISTIC_MARKER_EPOCH = 1793069981
CACHE = "/tmp/wz-tables.json"
TOOL = os.path.join(os.path.dirname(__file__), "..", "neon-calib", "tools", "wzread.py")
TABLES = ["Mob.img", "Skill.img", "Consume.img", "Npc.img", "Map.img", "Etc.img", "Ins.img", "Cash.img"]


def load_tables():
    if os.path.exists(CACHE):
        return {k: set(v) for k, v in json.load(open(CACHE)).items()}
    out = {}
    for t in TABLES:
        r = subprocess.run([sys.executable, TOOL, "names", t], capture_output=True, text=True)
        ids = set()
        for line in r.stdout.splitlines():
            k = line.split("\t")[0].strip()
            if k.isdigit():
                ids.add(int(k))
        out[t] = ids
        print(f"  loaded {t}: {len(ids)} ids", file=sys.stderr)
    json.dump({k: sorted(v) for k, v in out.items()}, open(CACHE, "w"))
    return out


def registry_u32_fields():
    text = open("neon-protocol/opcodes.toml").read()
    out = []
    for b in re.split(r"(?=\[\[opcode\]\])", text):
        idm = re.search(r"^id = (0x[0-9a-f]+)", b, re.M)
        dirm = re.search(r'^dir = "(\w+)"', b, re.M)
        if not (idm and dirm and dirm[1] == "in"):
            continue
        op = int(idm[1], 16)
        # Only the top-level field array: a [opcode.records] field's `off` is relative to each record, so
        # reading it at that absolute packet offset is nonsense (it briefly invented a "damage" skill id).
        top = b.split("[opcode.records]")[0]
        for fm in re.finditer(r'\{\s*name = "([^"]+)",\s*ty = "u32",\s*off = (\d+)', top):
            out.append((op, fm[1], int(fm[2])))
    return out


def discover(args, tables, ops):
    """Brute-force EVERY 4-byte offset of the given opcodes looking for unregistered content-id fields.

    skill_id@8 in the attack opcodes was found this way by accident; this makes it systematic. The same null
    model applies - a hit only counts if the values resolve far more often than a random value in that range
    would, which is what stops small-int fields (counts, damage, hp) being reported as ids."""
    seen = collections.defaultdict(lambda: collections.defaultdict(collections.Counter))
    for path in args:
        skipped = 0
        for line in open(path, errors="replace"):
            m = re.match(r"(\S+) op ([0-9a-f]{4}) len (\d+): ?(.*)", line)
            if not (m and m[4].strip()):
                continue
            skipped += 1
            if skipped <= 13 or float(m[1]) < HEURISTIC_MARKER_EPOCH:
                continue
            op = int(m[2], 16)
            if ops and op not in ops:
                continue
            b = bytes.fromhex(m[4].replace(" ", ""))
            for off in range(2, min(len(b) - 3, 400)):
                seen[op][off][struct.unpack_from("<I", b, off)[0]] += 1
    for op in sorted(seen):
        found = []
        for off, vals in seen[op].items():
            tot = sum(vals.values())
            if tot < 50 or len(vals) < 3:
                continue
            # Use the interdecile range, not min..max: ONE large outlier otherwise inflates the span, drives
            # the chance rate to ~0 and manufactures huge fake enrichment. (That happened: offset 4 of 0x02bb
            # overlaps char_id@2, so it reads char_id's high half - values 0..10, which land in Mob.img
            # trivially - and a single 50331650 outlier made it look like a 420,000x hit.)
            # 0 is a universal sentinel ("no skill", "no owner"); it resolves in nothing and would just drag
            # every share down, so judge on the non-zero values.
            vals = {v: c for v, c in vals.items() if v != 0}
            if len(vals) < 3:
                continue
            ordered = sorted(vals)
            lo = ordered[int(0.10 * (len(ordered) - 1))]
            hi = ordered[int(0.90 * (len(ordered) - 1))]
            span = max(hi - lo + 1, 1)
            for tname, ids in tables.items():
                # Judge on DISTINCT values, consistently with the range above. Mixing a packet-weighted hit
                # rate with a distinct-value range lets a couple of very frequent small values masquerade as a
                # whole field resolving (that is what offset 93 of 0x02aa did).
                hit = sum(1 for v in vals if v in ids)
                if not hit:
                    continue
                chance = sum(1 for i in ids if lo <= i <= hi) / span
                share = hit / len(vals)
                if share >= 0.9 and chance < 0.02 and share / chance >= 50:
                    found.append((share / chance, off, tname, share, len(vals), lo, hi))
        found.sort(reverse=True)
        print(f"{op:#06x}: {len(found)} candidate content-id offset(s)")
        for enr, off, tname, share, nd, lo, hi in found[:6]:
            print(f"    @{off:<4} {tname:<13} {share:5.1%} of values resolve, {nd} distinct, range {lo}..{hi}, enrichment {enr:,.0f}x")


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    only = next((a.split("=")[1] if "=" in a else None for a in sys.argv if a.startswith("--op")), None)
    if "--op" in sys.argv:
        only = sys.argv[sys.argv.index("--op") + 1]
    only = int(only, 0) if only else None

    tables = load_tables()
    if "--discover" in sys.argv:
        ops = {int(a, 0) for a in sys.argv if a.startswith("0x")}
        discover([a for a in args if not a.startswith("0x")], tables, ops)
        return
    fields = registry_u32_fields()
    if only:
        fields = [f for f in fields if f[0] == only]
    wanted = {f[0] for f in fields}
    seen = collections.defaultdict(collections.Counter)

    for path in args:
        skipped = 0
        for line in open(path, errors="replace"):
            m = re.match(r"(\S+) op ([0-9a-f]{4}) len (\d+): ?(.*)", line)
            if not (m and m[4].strip()):
                continue
            skipped += 1
            if skipped <= 13:
                continue
            if float(m[1]) < HEURISTIC_MARKER_EPOCH:
                continue
            op = int(m[2], 16)
            if op not in wanted:
                continue
            b = bytes.fromhex(m[4].replace(" ", ""))
            for o, name, off in fields:
                if o == op and off + 4 <= len(b):
                    seen[(op, name, off)][struct.unpack_from("<I", b, off)[0]] += 1

    print(f"{len(seen)} id-shaped field(s) with observed data\n")
    for (op, name, off), vals in sorted(seen.items(), key=lambda kv: -sum(kv[1].values())):
        tot = sum(vals.values())
        # NULL MODEL - without this the tool is worthless. Mob/Skill/Npc ids are small sequential integers, so
        # ANY field holding small integers (a damage factor, an hp value, a count) "resolves" in them by pure
        # coincidence. Measure instead how much better the field matches than a random value drawn from the
        # same range would: enrichment = observed match rate / (table ids falling in that range / range size).
        lo, hi = min(vals), max(vals)
        span = max(hi - lo + 1, 1)
        best = []
        for tname, ids in tables.items():
            hit = sum(c for v, c in vals.items() if v in ids)
            if not hit:
                continue
            in_range = sum(1 for i in ids if lo <= i <= hi)
            chance = in_range / span          # P(a random value in this range is in the table)
            share = hit / tot
            enr = share / chance if chance > 0 else float("inf")
            best.append((enr, share, tname, hit, chance))
        best.sort(reverse=True)
        head = f"{op:#06x} {name}@{off}  {tot} packets, {len(vals)} distinct, range {lo}..{hi}"
        if not best:
            print(f"{head}\n    resolves in NO table -> a runtime object id / handle, not a content id")
            print()
            continue
        print(head)
        for enr, share, tname, hit, chance in best[:3]:
            # A real content id resolves almost always AND would rarely do so by chance. A small-int field
            # resolves often but chance is also high, so enrichment stays near 1 and it is NOT flagged.
            if share >= 0.95 and enr >= 5:
                flag = "  <== CONFIRMED content id"
            elif share >= 0.95 and chance > 0.5:
                flag = "  (but chance is {:.0%} at this range - small-int coincidence, NOT evidence)".format(chance)
            else:
                flag = ""
            print(f"    {tname:<13} {hit}/{tot} = {share:5.1%}  chance {chance:5.1%}  enrichment {enr:5.1f}x{flag}")
        print()


if __name__ == "__main__":
    main()
