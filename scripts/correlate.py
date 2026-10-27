#!/usr/bin/env python3
"""Systematic correlation sweep: for every still-open incoming opcode (not already a confirmed/named event),
check whether its best id-bearing field lines up in TIME with a landmark event for that same id - a mob kill
(0x03d4), a player spawn (0x022d), or a player leave (0x0209). This generalizes, into a repeatable tool, the
one-off checks this project has done by hand for individual opcodes (0x0441 vs mob kills, 0x02bb/0x0279/etc vs
PlayerSpawn) so new candidates can be found by running one script over the WHOLE todo list instead of picking one
opcode at a time by eye.

  scripts/correlate.py LOG...

Only reports opcodes whose own registry entry does NOT already mention a kill/spawn/leave timing check (crude text
search), so it does not re-surface findings that are already written up. Report only; writes nothing to
opcodes.toml - a human (or a future pass) decides whether a hit is strong enough to write up, same discipline as
everywhere else in this project (a correlation is a lead, not a proof).
"""
import collections, re, struct, sys

logs = [a for a in sys.argv[1:] if not a.startswith("--")]
WINDOW_S = float(next((a.split("=")[1] for a in sys.argv if a.startswith("--window=")), 10.0))
HEURISTIC_MARKER_EPOCH = 1793069981  # see registry-facts.py for why

pk = collections.defaultdict(list)       # op -> [(t, bytes)]
kills = []                               # (t, mob_oid)
spawns = []                              # (t, char_id)
leaves = []                              # (t, char_id)
for path in logs:
    skipped = 0
    for line in open(path, errors="replace"):
        m = re.match(r"(\S+) op (\w+) len (\d+): ?(.*)", line)
        if m and m[4].strip():
            skipped += 1
            if skipped <= 13:
                continue
            t = float(m[1])
            if t < HEURISTIC_MARKER_EPOCH:
                continue
            op = int(m[2], 16)
            b = bytes.fromhex(m[4].replace(" ", ""))
            pk[op].append((t, b))
            if op == 0x03d4 and len(b) >= 6:
                kills.append((t, struct.unpack_from("<I", b, 2)[0]))
            elif op == 0x022d and len(b) >= 10:
                spawns.append((t, struct.unpack_from("<I", b, 6)[0]))
            elif op == 0x0209 and len(b) == 13:
                leaves.append((t, struct.unpack_from("<I", b, 2)[0]))

kills_by_id = collections.defaultdict(list)
for t, oid in kills:
    kills_by_id[oid].append(t)
spawns_by_id = collections.defaultdict(list)
for t, cid in spawns:
    spawns_by_id[cid].append(t)
leaves_by_id = collections.defaultdict(list)
for t, cid in leaves:
    leaves_by_id[cid].append(t)

def near(times, t, window):
    return any(abs(t - x) <= window for x in times)

def best_id_offset(rows):
    """The u32 offset (at the dominant length) with the most distinct, large-looking values - a candidate
    mob_oid/char_id field, regardless of whether those values are already in a known-id set (unlike
    registry-facts.py's best_ref, which only counts hits against players/mobs already seen in OTHER opcodes -
    here we want ANY offset that looks like an id, so a kill/spawn-only field is not skipped for being
    "unverified" elsewhere)."""
    lens = collections.Counter(len(b) for t, b in rows)
    if not lens:
        return None
    dom, cnt = lens.most_common(1)[0]
    if cnt < 10 or dom < 6:
        return None
    same = [(t, b) for t, b in rows if len(b) == dom]
    best = None
    for o in range(2, min(dom - 3, 120)):
        vals = [struct.unpack_from("<I", b, o)[0] for t, b in same]
        large = sum(1 for v in vals if v >= 100_000)
        if large / len(vals) < 0.8 or len(set(vals)) < 5:
            continue
        if best is None or large > best[1]:
            best = (o, large)
    return best[0] if best else None

text = open("neon-protocol/opcodes.toml").read()
parts = re.split(r"(?=\[\[opcode\]\])", text)
already_checked = set()
for b in parts:
    idm = re.search(r'^id = (0x[0-9a-f]+)\n', b, re.M)
    if not idm:
        continue
    named = re.search(r'^name = ', b, re.M) is not None
    # crude, deliberately generous: an entry that names a landmark opcode's id directly (0x03d4 kill, 0x022d
    # spawn, 0x0209 leave) has, in this project's practice so far, always done so because someone already
    # related this opcode to that landmark by hand or by this tool - never in passing. Broader than matching
    # correlate.py's own output phrasing, so it also recognizes pre-existing hand-written analysis like 0x022e's
    # (compared against "0x0209 PlayerLeave" with its own gap study, well before this tool existed).
    checked_phrases = ("mob kill", "0x03d4", "0x022d", "0x0209", "within 2s by a", "a leave-triggered broadcast")
    mentions = any(p in b.lower() for p in checked_phrases)
    if named or mentions:
        already_checked.add(int(idm[1], 16))

print(f"{len(kills)} kill events, {len(spawns)} spawn events, {len(leaves)} leave events, {len(already_checked)} opcodes already have a documented timing check")
results = []
for op, rows in pk.items():
    if op in already_checked or op in (0x03d4, 0x022d, 0x0209):
        continue
    off = best_id_offset(rows)
    if off is None:
        continue
    lens = collections.Counter(len(b) for t, b in rows)
    dom = lens.most_common(1)[0][0]
    same = [(t, b) for t, b in rows if len(b) == dom]
    for label, by_id in (("kill", kills_by_id), ("spawn", spawns_by_id), ("leave", leaves_by_id)):
        hits = sum(1 for t, b in same if near(by_id.get(struct.unpack_from("<I", b, off)[0], []), t, WINDOW_S))
        share = hits / len(same)
        if share >= 0.5 and hits >= 10:
            results.append((share, hits, len(same), op, off, label))

results.sort(reverse=True)
print(f"\n{len(results)} candidate(s) with a u32 field that lines up with a kill/spawn/leave event within {WINDOW_S}s at least half the time:")
for share, hits, total, op, off, label in results[:20]:
    print(f"  {op:#06x}  u32@{off}  {hits}/{total} ({share:.0%}) near a {label} event")
