#!/usr/bin/env python3
"""Check the documented layout of 0x044d (mob damaged) against a stamped packet log.

  verify_dmg.py run/view.NAME.log [own char id]

Claim under test: oid u32@2 is a mob (seen in 0x03c9 spawn @3 or 0x03dc move @2), attacker u32@6 is a character id,
damage u32@10 is a plausible number. Prints the evidence; it does not decide for you.
"""
import collections, re, struct, sys

log = sys.argv[1]
own = int(sys.argv[2]) if len(sys.argv) > 2 else None
mobs, dmg = set(), []
for line in open(log, errors="replace"):
    m = re.match(r"\S+ op (\w+) len (\d+): ?(.*)", line)
    if not m:
        continue
    op, b = m[1], bytes.fromhex(m[3].replace(" ", "")) if m[3].strip() else b""
    if op == "03c9" and len(b) >= 7:
        mobs.add(struct.unpack_from("<I", b, 3)[0])
    elif op == "03dc" and len(b) >= 6:
        mobs.add(struct.unpack_from("<I", b, 2)[0])
    elif op == "044d" and len(b) == 14:
        dmg.append((struct.unpack_from("<I", b, 2)[0], struct.unpack_from("<I", b, 6)[0], struct.unpack_from("<I", b, 10)[0], b))
print(f"{len(dmg)} packets of 0x044d (14 bytes); {len(mobs)} distinct mob oids seen")
if not dmg:
    sys.exit(0)
known = sum(1 for o, a, d, b in dmg if o in mobs)
print(f"oid is a mob we saw: {known}/{len(dmg)} ({100 * known // len(dmg)}%)")
att = collections.Counter(a for o, a, d, b in dmg)
print("attackers:", att.most_common(5))
if own:
    print(f"attacker == own id {own}: {att.get(own, 0)}")
ds = sorted(d for o, a, d, b in dmg)
print("damage min/median/max:", ds[0], ds[len(ds) // 2], ds[-1], "| over 100000:", sum(1 for d in ds if d > 100000))
print("first bytes of 3 packets:", [b.hex(" ") for o, a, d, b in dmg[:3]])
