#!/usr/bin/env python3
"""Extract item names from the client's String_000.wz into neon-calib/items.json (id -> name), the
same lookup wzstrings.py already builds for maps/mobs and npc-names.json already holds for NPCs.

  wzitems.py <String_000.wz> <out.json>

Used by the `keep_items` field of a plan's `shop` step (see docs/features.md and
docs/warrior-10-30.md): a plan can name an item instead of a sell-list row, and the id is looked
up here once and written into the plan file, since the bot itself only ever sees ids (from the
server's inventory updates), never names.
"""
import json, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wzmap as wz

# String.wz's own split of item names, one .img per category (same split the client's item ids
# use for their leading digit: 1xxxxxx equip, 2xxxxxx consume, 3xxxxxx install/etc-ish, 4xxxxxx
# etc, 5xxxxxx cash).
CATEGORY_FILES = ["Eqp.img", "Con.img", "Ins.img", "Etc.img", "Pet.img", "Cash.img"]


def main():
    src, out = sys.argv[1], sys.argv[2]
    r, imgs = wz.open_wz(src)
    by = {os.path.basename(p): off for p, off, _ in imgs}
    items = {}
    for fname in CATEGORY_FILES:
        if fname not in by:
            continue
        tree = wz.parse_img(r, by[fname])
        for category, entries in tree.items():
            if not isinstance(entries, dict):
                continue
            for iid, v in entries.items():
                if isinstance(v, dict) and iid.isdigit():
                    name = v.get("name", "")
                    if name:
                        items[int(iid)] = name
    json.dump(items, open(out, "w"), ensure_ascii=False, indent=0, sort_keys=True)
    print(len(items), "items")


main()
