#!/usr/bin/env python3
"""Search every opcode for fields that equal a *property of the entity the packet refers to*.

  autocorr.py <packet log> [min packets, default 40] [min share, default 0.9]

Entities come from verified packets: mobs (0x03c9/0x03d5 spawn: template, HP, position; 0x03dc moves update the position),
players (0x022d spawn: level; 0x029d moves: position). For each opcode and each 4-byte id-reference field (a field holding
known mob or player ids in >= 60% of packets) the tool tests every other offset, as u8/u16/u32/i16/i32, against the entity's
attributes: template id, spawn HP, level / EXP from neon-calib/mob-info.json (public database), last known x and y,
and for players the level and last position. A match in >= min share of packets, with at least 3 distinct values, is printed.
It reports shape and evidence only: a person decides whether it deserves a name.
"""
import collections, json, re, struct, sys

log = sys.argv[1]
min_n = int(sys.argv[2]) if len(sys.argv) > 2 else 40
min_share = float(sys.argv[3]) if len(sys.argv) > 3 else 0.9
db = json.load(open("neon-calib/mob-info.json")).get("mobs", {})

mobs, players = {}, {}
packets = collections.defaultdict(list)  # op -> [(bytes, snapshot of entity tables as of this packet)]

def end_pos(b, start, tail, count_at, base):
    if len(b) > count_at and len(b) >= start + tail and (len(b) - start - tail) % 21 == 0 and b[count_at] == (len(b) - start - tail) // 21 and b[count_at]:
        r = start + 21 * (b[count_at] - 1)
        return struct.unpack_from("<hh", b, r + 2)
    return struct.unpack_from("<hh", b, base)

KNOWN = {"03c9", "03d5", "03dc", "022d", "029d", "0282", "0456"}
for line in open(log, errors="replace"):
    m = re.match(r"\S+ op (\w+) len \d+: ?(.*)", line)
    if not m or not m[2].strip():
        continue
    op, b = m[1], bytes.fromhex(m[2].replace(" ", ""))
    try:
        if op in ("03c9", "03d5") and len(b) >= 313:
            oid, tpl = struct.unpack_from("<II", b, 3)[0], struct.unpack_from("<I", b, 8)[0]
            x, y = struct.unpack_from("<hh", b, 206)
            mobs[oid] = {"template": tpl, "hp": struct.unpack_from("<I", b, 225)[0], "x": x, "y": y}
        elif op == "03dc" and len(b) >= 34:
            oid = struct.unpack_from("<I", b, 2)[0]
            if oid in mobs:
                mobs[oid]["x"], mobs[oid]["y"] = end_pos(b, 39, 2, 38, 30)
        elif op == "022d" and len(b) >= 24:
            n = struct.unpack_from("<H", b, 18)[0]
            if 1 <= n <= 16:
                players[struct.unpack_from("<I", b, 6)[0]] = {"level": struct.unpack_from("<I", b, 14)[0]}
        elif op == "029d" and len(b) >= 14:
            pid = struct.unpack_from("<I", b, 2)[0]
            p = players.setdefault(pid, {})
            p["x"], p["y"] = end_pos(b, 19, 1, 18, 10)
    except struct.error:
        pass
    if op in KNOWN:
        continue
    packets[op].append((b, {k: dict(v) for k, v in ((i, mobs.get(i)) for i in ())}, ))
    # keep references cheap: store raw bytes and resolve attributes lazily with a copy of the two relevant dicts at that moment
    packets[op][-1] = (b, dict(mobs), dict(players)) if len(packets[op]) <= 400 else packets[op][-1]

def attrs(kind, ent):
    if kind == "mob":
        t = db.get(str(ent["template"]), {})
        a = {"template": ent["template"], "hp": ent["hp"], "x": ent["x"], "y": ent["y"]}
        if t:
            a.update({"db_level": t["level"], "db_exp": t["exp"], "db_hp": t["hp"]})
        return a
    return {k: v for k, v in ent.items()}

found = 0
for op, rows in sorted(packets.items(), key=lambda kv: -len(kv[1])):
    rows = [r for r in rows if len(r) == 3]
    if len(rows) < min_n:
        continue
    L = collections.Counter(len(r[0]) for r in rows).most_common(1)[0][0]
    rows = [r for r in rows if len(r[0]) == L]
    if len(rows) < min_n:
        continue
    for off in range(2, L - 3):
        for kind in ("mob", "player"):
            ok = [(r, struct.unpack_from("<I", r[0], off)[0]) for r in rows]
            table = lambda r: r[1] if kind == "mob" else r[2]
            ref = [(r, v) for r, v in ok if v in table(r)]
            if len(ref) / len(rows) < 0.6 or len({v for _, v in ref}) < 3:
                continue
            # every other offset against the referenced entity's attributes
            for off2 in range(2, L - 1):
                if off <= off2 < off + 4 or off2 <= off < off2 + 4 and False:
                    continue
                for fmt, w in (("<B", 1), ("<H", 2), ("<h", 2), ("<I", 4), ("<i", 4)):
                    if off2 + w > L:
                        continue
                    hits = collections.Counter()
                    vals = set()
                    for r, v in ref:
                        ent = table(r)[v]
                        val = struct.unpack_from(fmt, r[0], off2)[0]
                        vals.add(val)
                        for name, av in attrs(kind, ent).items():
                            if av == val:
                                hits[name] += 1
                    if len(vals) < 3:
                        continue
                    for name, h in hits.items():
                        if h / len(ref) >= min_share:
                            found += 1
                            print(f"0x{op} (len {L}): {fmt[1:]} @{off2} equals the {kind}'s {name} in {h}/{len(ref)} packets (the {kind} id is @{off}; {len(vals)} distinct values)")
if not found:
    print("nothing matched")
