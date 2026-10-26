#!/usr/bin/env python3
"""Read every player-position source in chains.json at once and say which agree (read-only).

  possources.py <pid> [maps_dir]

Prints the i32 sources (player_position, player_position_alt) and each f32 chain, the map id, and, with a
maps dir, whether each value stands on one of that map's footholds (within 2 px). Run it after map changes and
restarts: a source that freezes at an old position, or leaves the floor, is not the player.
"""
import json
import os
import struct
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))


def main():
    pid = int(sys.argv[1])
    maps = sys.argv[2] if len(sys.argv) > 2 else os.path.join(ROOT, "neon-calib", "maps")
    mem = os.open(f"/proc/{pid}/mem", os.O_RDONLY)
    c = json.load(open(os.path.join(ROOT, "neon-calib", "chains.json")))

    def u64(a):
        return struct.unpack("<Q", os.pread(mem, 8, a))[0]

    def follow(root, offs):
        a = u64(root) + offs[0]
        for k in offs[1:]:
            a = u64(a) + k
        return a

    ch = c["chains"]
    map_id = None
    try:
        st = ch["player_state"]
        off = st["fields"]["map_id"]["offset"]
        off = -int(off[1:], 16) if off.startswith("-") else int(off, 16)
        map_id = struct.unpack("<I", os.pread(mem, 4, u64(int(st["root"], 16)) + off))[0]
    except Exception:
        pass
    floors = []
    if map_id:
        p = os.path.join(maps, f"{map_id}.json")
        if os.path.exists(p):
            floors = [f for f in json.load(open(p))["footholds"] if f["x1"] != f["x2"]]

    def on_floor(x, y):
        for f in floors:
            x1, x2 = sorted((f["x1"], f["x2"]))
            if x1 <= x <= x2:
                t = 0 if f["x2"] == f["x1"] else (x - f["x1"]) / (f["x2"] - f["x1"])
                if abs(f["y1"] + t * (f["y2"] - f["y1"]) - y) <= 2:
                    return True
        return False

    rows = []
    for key in ("player_position", "player_position_alt"):
        s = ch.get(key)
        if not s:
            continue
        try:
            base = u64(int(s["root"], 16))
            x, y = struct.unpack("<2i", os.pread(mem, 8, base + int(s["x_offset"], 16)))
            rows.append((key, x, y))
        except Exception as e:
            rows.append((key, None, str(e)))
    for i, s in enumerate(ch.get("player_position_f32", {}).get("chains", [])):
        try:
            a = follow(int(s["root"], 16), [int(o, 16) for o in s["offsets"]])
            x, y = struct.unpack("<2f", os.pread(mem, 8, a))
            rows.append((f"f32[{i}] {s['root']} {'+'.join(s['offsets'])}", round(x), round(y)))
        except Exception as e:
            rows.append((f"f32[{i}]", None, str(e)))
    print(f"map {map_id} ({len(floors)} floors)")
    for name, x, y in rows:
        flag = "" if x is None or not floors else ("on floor" if on_floor(x, y) else "OFF FLOOR")
        print(f"  {name:<48} {x!s:>6} {y!s:>6}  {flag}")


if __name__ == "__main__":
    main()
