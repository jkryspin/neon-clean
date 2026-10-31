#!/usr/bin/env python3
"""Extract map and mob names from the client's String_000.wz.
  wzstrings.py <String_000.wz> <outdir>   ->  <outdir>/map-names.json, <outdir>/mob-names.json"""
import importlib.util, json, os, sys

spec = importlib.util.spec_from_file_location('wz', os.path.join(os.path.dirname(os.path.abspath(__file__)), 'wzmap.py'))
wz = importlib.util.module_from_spec(spec); spec.loader.exec_module(wz)

def main():
    src, out = sys.argv[1], sys.argv[2]
    r, imgs = wz.open_wz(src)
    by = {os.path.basename(p): off for p, off, _ in imgs}
    maps = {}
    tree = wz.parse_img(r, by['Map.img'])
    for category, entries in tree.items():
        if not isinstance(entries, dict): continue
        for mid, v in entries.items():
            if isinstance(v, dict) and mid.isdigit():
                maps[int(mid)] = {'name': v.get('mapName', ''), 'street': v.get('streetName', ''), 'region': category}
    mobs = {}
    tree = wz.parse_img(r, by['Mob.img'])
    for mid, v in tree.items():
        if isinstance(v, dict) and mid.isdigit():
            mobs[int(mid)] = v.get('name', '')
    json.dump(maps, open(os.path.join(out, 'map-names.json'), 'w'), ensure_ascii=False, indent=0, sort_keys=True)
    json.dump(mobs, open(os.path.join(out, 'mob-names.json'), 'w'), ensure_ascii=False, indent=0, sort_keys=True)
    print(len(maps), 'maps,', len(mobs), 'mobs')

main()
