#!/usr/bin/env python3
"""Compare the shadow simulation with the real character and fit the movement constants.

Input: one or more `--sim-log` files from neon-loader (live `--sim-log FILE`, or offline
`--sim-replay TRACE MAP --sim-log FILE`). Columns:
  ts dt dir jump down climb ax ay agr sx sy sgr age err

Prints, from the REAL positions only:
  * walking speed (grounded, a direction held, measured over >= 200 ms windows)
  * jump arcs: rise, time to peak, sideways speed in the air
  * fastest fall
and, for the shadow, the position error by time since its last reset (how fast the model drifts).
Frames where the memory read jumps implausibly (> 60 px in one frame) are dropped as read glitches.
"""
import statistics as st
import sys


def load(path):
    rows = []
    with open(path) as f:
        next(f, None)
        for line in f:
            p = line.split()
            if len(p) != 14:
                continue
            rows.append({
                "ts": int(p[0]), "dt": int(p[1]), "dir": p[2], "jump": p[3] == "1", "down": p[4] == "1",
                "climb": p[5] == "1", "ax": int(p[6]), "ay": int(p[7]), "agr": p[8],
                "sx": float(p[9]), "sy": float(p[10]), "sgr": p[11] == "1", "age": int(p[12]), "err": float(p[13]),
            })
    return rows


def fresh(rows):
    """Frames where the position read changed. The client's position updates in bursts (two new
    values, then ~60 ms of repeats), so only these frames say where the character is now. Inputs
    of skipped frames are merged into the next kept one."""
    out, last, jump, down = [], None, False, False
    for r in rows:
        jump |= r["jump"]
        down |= r["down"]
        if last and (r["ax"], r["ay"]) == last:
            continue
        r = dict(r, jump=jump, down=down)
        jump = down = False
        out.append(r)
        last = (r["ax"], r["ay"])
    return out


def clean(rows):
    out, prev = [], None
    for r in rows:
        if prev and r["ts"] - prev["ts"] <= 100 and (abs(r["ax"] - prev["ax"]) > 60 or abs(r["ay"] - prev["ay"]) > 60):
            prev = r
            continue  # read glitch
        out.append(r)
        prev = r
    return out


def q(xs, f):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(f * len(xs)))] if xs else float("nan")


def walk_speed(rows):
    v = []
    for i, r in enumerate(rows):
        if r["agr"] != "1" or r["dir"] == "-" or r["jump"] or r["climb"]:
            continue
        # Find the frame >= 200 ms later with the same input, still grounded throughout.
        for j in range(i + 1, min(len(rows), i + 40)):
            s = rows[j]
            if s["agr"] != "1" or s["dir"] != r["dir"] or s["jump"] or s["climb"]:
                break
            if s["ts"] - r["ts"] >= 200:
                dx = (s["ax"] - r["ax"]) * (1 if r["dir"] == "R" else -1)
                v.append(dx * 1000 / (s["ts"] - r["ts"]))
                break
    return v


def jump_arcs(rows):
    arcs = []
    for i, r in enumerate(rows):
        if not r["jump"] or r["down"] or i == 0:
            continue
        o = rows[i - 1]  # the key went down just before this frame was read
        if o["agr"] != "1":
            continue
        x0, y0, t0 = o["ax"], o["ay"], o["ts"]
        pts = []
        airborne = False
        for s in rows[i:i + 80]:
            t = (s["ts"] - t0) / 1000
            pts.append((t, s["ax"] - x0, s["ay"] - y0))
            if s["agr"] == "0":
                airborne = True
            elif airborne and s["agr"] == "1":
                break
            if t > 1.2:
                break
        if not airborne or len(pts) < 5:
            continue
        tp, _, yp = min(pts, key=lambda p: p[2])
        side = [(t, x) for t, x, _ in pts if t <= tp + 0.15]
        air = [(t, x) for t, x, y in pts if y < -3]
        vx = None
        if len(air) >= 3 and air[-1][0] > air[0][0]:
            vx = abs(air[-1][1] - air[0][1]) / (air[-1][0] - air[0][0])
        arcs.append({"rise": -yp, "tpeak": tp, "vx": vx, "held": r["dir"], "land_t": pts[-1][0], "land_dy": pts[-1][2], "step": o.get("step")})
    return arcs


def fall_speed(rows):
    v = []
    for a, b in zip(rows, rows[1:]):
        if a["agr"] == "0" and b["agr"] == "0" and 0 < b["ts"] - a["ts"] <= 60 and b["ay"] > a["ay"]:
            v.append((b["ay"] - a["ay"]) * 1000 / (b["ts"] - a["ts"]))
    return v


def landings(rows):
    """For every jump/drop: the real and simulated feet when the real character lands again."""
    out = []
    i = 0
    while i < len(rows):
        r = rows[i]
        if not r["jump"]:
            i += 1
            continue
        start = r
        airborne, j = False, i + 1
        while j < len(rows) and rows[j]["ts"] - start["ts"] < 2500:
            s = rows[j]
            if s["agr"] == "0":
                airborne = True
            elif airborne and s["agr"] == "1":
                break
            j += 1
        if airborne and j < len(rows):
            e = rows[j]
            out.append({"t": start["ts"], "kind": "drop" if start["down"] else f"jump {start['dir']}",
                        "from": (start["ax"], start["ay"]), "real": (e["ax"], e["ay"]), "sim": (round(e["sx"]), round(e["sy"])),
                        "dx": round(e["sx"] - e["ax"]), "dy": round(e["sy"] - e["ay"])})
        i = max(j, i + 1)
    return out


def main(paths):
    rows = []
    for p in paths:
        rows += fresh(clean(load(p)))
    print(f"{len(rows)} frames from {len(paths)} log(s)")

    w = walk_speed(rows)
    if w:
        print(f"walk speed: median {st.median(w):.0f} px/s  (p10 {q(w, .1):.0f}, p90 {q(w, .9):.0f}, n={len(w)})")

    arcs = jump_arcs(rows)
    if arcs:
        rise = [a["rise"] for a in arcs if a["rise"] > 20]
        tpk = [a["tpeak"] for a in arcs if a["rise"] > 20]
        vxs = [a["vx"] for a in arcs if a["vx"] is not None and a["held"] != "-" and a["rise"] > 20]
        print(f"jumps: {len(arcs)}  rise median {st.median(rise):.0f} px (p10 {q(rise, .1):.0f}, p90 {q(rise, .9):.0f}),"
              f" time to peak {st.median(tpk):.2f} s")
        if vxs:
            print(f"  sideways in the air (direction held): median {st.median(vxs):.0f} px/s (p10 {q(vxs, .1):.0f}, p90 {q(vxs, .9):.0f})")
        if rise and tpk:
            r, t = st.median(rise), st.median(tpk)
            print(f"  => jump_v0 {2 * r / t:.0f} px/s, gravity {2 * r / t / t:.0f} px/s^2")

    f = fall_speed(rows)
    if f:
        print(f"falling: p90 {q(f, .9):.0f} px/s, max {max(f):.0f} px/s")

    print("shadow error by time since reset (median / p90 px):")
    for lo, hi in [(0, 250), (250, 500), (500, 1000), (1000, 2000), (2000, 10**9)]:
        for mode, pick in [("ground", True), ("air", False)]:
            e = [r["err"] for r in rows if lo <= r["age"] < hi and r["sgr"] == pick]
            if e:
                print(f"  {lo:>5}-{hi if hi < 10**9 else '':<5} ms {mode:<6} {st.median(e):6.1f} / {q(e, .9):6.1f}  (n={len(e)})")
    ls = landings(rows)
    if ls:
        wrong_floor = [l for l in ls if abs(l["dy"]) > 12]
        print(f"landings: {len(ls)}, sim on the wrong floor {len(wrong_floor)}, median |dx| {st.median(abs(l['dx']) for l in ls):.0f} px")
        for l in ls:
            flag = "  <-- floor" if abs(l["dy"]) > 12 else ""
            print(f"  {l['kind']:<8} from {l['from']} real {l['real']} sim {l['sim']} dx {l['dx']:+} dy {l['dy']:+}{flag}")
    resets = sum(1 for r in rows if r["age"] == 0)
    print(f"resets: {resets}")


if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    main(sys.argv[1:])
