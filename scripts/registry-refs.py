#!/usr/bin/env python3
"""Record in the registry which unknown opcodes carry the id of a known mob / player, and where.

  scripts/registry-refs.py <packet log> [min share, default 0.8]

Runs neon-calib/tools/idscan.py, and for every (opcode, offset, kind) it reports that the registry does not already describe, adds a
field to the opcode's primary incoming entry (`char_id` or `mob_oid`) and upgrades an `observed` entry to `structure`. It never
renames anything and never touches entries that already have a field at that offset. The summary gets one sentence with the numbers.
Run the tests afterwards (scripts/test-headless.sh): the registry validates itself.
"""
import re, subprocess, sys, tomllib

log = sys.argv[1]
share = sys.argv[2] if len(sys.argv) > 2 else "0.8"
out = subprocess.run([sys.executable, "neon-calib/tools/idscan.py", log, "8", share], capture_output=True, text=True).stdout
refs = []
for line in out.splitlines():
    m = re.match(r"0x([0-9a-f]{4}) len (\d+): u32@(\d+) is a known (mob|player|drop) id in (\d+)/(\d+) packets \((\d+) distinct", line)
    if m and m[4] != "drop":
        refs.append((int(m[1], 16), int(m[2]), int(m[3]), m[4], int(m[5]), int(m[6])))
path = "neon-protocol/opcodes.toml"
text = open(path).read()
reg = tomllib.loads(text)["opcode"]
added = 0
for op, ln, off, kind, hit, n in refs:
    group = [o for o in reg if o["dir"] == "in" and o["id"] == op]
    if any(f["off"] == off for o in group for f in o.get("field", [])):
        continue  # already described
    name = "char_id" if kind == "player" else "mob_oid"
    m = re.search(r'\n\[\[opcode\]\]\nid = 0x%04x\ndir = "in"\n.*?(?=\n\[\[opcode\]\]|\Z)' % op, text, re.S)
    if m:
        blk = m.group(0)
        # Only unexplored entries: observed/structure with no fields, no variants, no records, and no size bound that the field would break.
        if any(w in blk for w in ("records", "when =", "field = [")) or not re.search(r'status = "(observed|structure)"', blk):
            continue
        lm = re.search(r"len_min = (\d+)", blk)
        if lm and int(lm.group(1)) < off + 4:
            continue
        new = blk.replace('status = "observed"', 'status = "structure"')
        sentence = f"u32@{off} is a known {kind} id in {hit} of {n} packets of the {ln}-byte form (idscan, pooled captures 2026-10-26)."
        if "summary = " in new:
            new = re.sub(r'summary = "([^"\n]*?)\.?"', lambda mm: f'summary = "{mm.group(1)}. {sentence}"', new, count=1)
        else:
            new = new.replace('status = "structure"\n', f'status = "structure"\nsummary = "{sentence}"\n', 1)
        new = new.rstrip("\n") + f'\nfield = [{{ name = "{name}", ty = "u32", off = {off} }}]\n'
        if "len_min" not in new:
            new += f"len_min = {off + 4}\n"
        text = text.replace(blk, new, 1)
    else:
        text += f'\n[[opcode]]\nid = 0x{op:04x}\ndir = "in"\nstatus = "structure"\nsummary = "u32@{off} is a known {kind} id in {hit} of {n} packets of the {ln}-byte form (idscan, pooled captures 2026-10-26)."\nlen_min = {off + 4}\nfield = [{{ name = "{name}", ty = "u32", off = {off} }}]\n'
    added += 1
open(path, "w").write(text)
tomllib.loads(text)
print(f"{len(refs)} references found, {added} new fields added to the registry")
