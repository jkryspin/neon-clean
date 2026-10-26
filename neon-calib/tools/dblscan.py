#!/usr/bin/env python3
"""Find the client's own physics position: adjacent (x, y) float64 pairs that follow the character (read-only).

  dblscan.py <pid> <status.json> [rounds]

The status file is a running bot's neon-status.<session>.json (its player x/y is the stack-slot read). Round 1 keeps
every 4-byte-aligned double pair within TOL px of that position; each later round, taken after the character has
moved, keeps only the candidates that still match. Survivors are printed with the 32 bytes that follow them (the
velocity usually sits right after the position in the client's vector controller).
"""
# DBL_DRIVE=1 walks the character a little (Right/Left alternately) between rounds and compares while it stands.
# DBL_MODE=f64 or f32 looks for a lone x value instead of an (x, y) double pair.
import json
import os
import sys
import time

import numpy as np

TOL = float(os.environ.get("DBL_TOL", "60"))


LATEST = {}


def xdo(*args):
    import subprocess
    env = dict(os.environ)
    env.setdefault("DISPLAY", ":0")
    if "XAUTHORITY" not in env:
        d = f"/run/user/{os.getuid()}"
        for f in os.listdir(d):
            if f.startswith(".mutter-Xwaylandauth."):
                env["XAUTHORITY"] = os.path.join(d, f)
    subprocess.run(["xdotool", *args], env=env, check=False)


def follow_stream(pid):
    """Keep the newest stack-slot position from our own neon-stream (the status file lags ~1 s)."""
    import subprocess
    import threading
    root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    proc = subprocess.Popen([f"{root}/neon-stream/target/release/neon-stream", "--pid", str(pid), "--chains",
                             f"{root}/neon-calib/chains.json", "--no-footholds"], stdout=subprocess.PIPE,
                            stderr=subprocess.DEVNULL, text=True)

    def run():
        for line in proc.stdout:
            try:
                p = json.loads(line)["player"]
                LATEST["pos"] = (float(p["x"]), float(p["y"]))
            except (ValueError, KeyError, TypeError):
                pass
    threading.Thread(target=run, daemon=True).start()
    while "pos" not in LATEST:
        time.sleep(0.05)
    return proc


def player(_path):
    return LATEST["pos"]


def regions(pid):
    out = []
    for line in open(f"/proc/{pid}/maps"):
        p = line.split()
        if not p[1].startswith("rw") or "dri" in line or "nvidia" in line:
            continue
        a, b = [int(x, 16) for x in p[0].split("-")]
        out.append((a, b))
    return out


CHUNK = 4 << 20


def chunks(mem, pid):
    """(address, bytes) of all writable memory in 4 MB pieces (16 bytes overlap), pausing between them: one big
    read of /proc/pid/mem holds the game's memory lock long enough to stall it (a scan disconnected the client)."""
    for a, b in regions(pid):
        for c in range(a, b, CHUNK):
            n = min(CHUNK + 16, b - c)
            try:
                d = os.pread(mem, n, c)
            except OSError:
                continue
            yield c, d
            time.sleep(0.003)


def first_round(mem, pid, x, y):
    if MODE != "pair":
        return first_round_single(mem, pid, x)
    cands = []
    for a, d in chunks(mem, pid):
        for shift in (0, 4):
            n = (len(d) - shift) // 8
            if n < 2:
                continue
            v = np.frombuffer(d, dtype="<f8", count=n, offset=shift)
            with np.errstate(invalid="ignore"):
                ok = (np.abs(v[:-1] - x) <= TOL) & (np.abs(v[1:] - y) <= TOL)
            for i in np.nonzero(ok)[0]:
                cands.append(a + shift + 8 * int(i))
    return cands


MODE = os.environ.get("DBL_MODE", "pair")  # pair | f64 | f32: what to look for


def first_round_single(mem, pid, x):
    """Every lone 4-byte-aligned double (f64) or float (f32) within TOL of x."""
    cands = []
    for a, d in chunks(mem, pid):
        if MODE == "f64":
            for shift in (0, 4):
                n = (len(d) - shift) // 8
                if n < 1:
                    continue
                v = np.frombuffer(d, dtype="<f8", count=n, offset=shift)
                with np.errstate(invalid="ignore"):
                    ok = np.abs(v - x) <= TOL
                cands += [a + shift + 8 * int(i) for i in np.nonzero(ok)[0]]
        else:
            n = len(d) // 4
            v = np.frombuffer(d, dtype="<f4", count=n)
            with np.errstate(invalid="ignore"):
                ok = np.abs(v - x) <= TOL
            cands += [a + 4 * int(i) for i in np.nonzero(ok)[0]]
    return cands


def read_one(mem, addr):
    try:
        if MODE == "f64":
            return float(np.frombuffer(os.pread(mem, 8, addr), dtype="<f8")[0])
        return float(np.frombuffer(os.pread(mem, 4, addr), dtype="<f4")[0])
    except OSError:
        return None


def read_pair(mem, addr):
    try:
        return np.frombuffer(os.pread(mem, 16, addr), dtype="<f8")
    except OSError:
        return None


def main():
    pid, status = int(sys.argv[1]), sys.argv[2]
    rounds = int(sys.argv[3]) if len(sys.argv) > 3 else 8
    os.nice(19)
    mem = os.open(f"/proc/{pid}/mem", os.O_RDONLY)
    stream = follow_stream(pid)
    import atexit
    atexit.register(stream.terminate)
    x, y = player(status)
    cands = sorted(set(first_round(mem, pid, x, y)))
    print(f"round 1 at ({x:.0f}, {y:.0f}): {len(cands)} candidates", flush=True)
    last = (x, y)
    r = 1
    drive = os.environ.get("DBL_DRIVE") == "1"
    step = 0
    while r < rounds and cands:
        if drive:
            # Walk a little (alternating, so we stay put overall), then stand still for the comparison.
            key = "Right" if step % 2 == 0 else "Left"
            step += 1
            xdo("keydown", key)
            time.sleep(float(os.environ.get("DBL_STEP", "0.35")))
            xdo("keyup", key)
            time.sleep(0.6)
        else:
            time.sleep(0.7)
        x, y = player(status)
        if abs(x - last[0]) + abs(y - last[1]) < (20 if drive else 30):
            print(f"  (not moved: {x:.0f},{y:.0f})", flush=True)
            continue  # wait until the character has moved
        last = (x, y)
        r += 1
        keep = []
        for c in cands:
            if MODE == "pair":
                v = read_pair(mem, c)
                if v is not None and abs(v[0] - x) <= TOL and abs(v[1] - y) <= TOL:
                    keep.append(c)
            else:
                v = read_one(mem, c)
                if v is not None and abs(v - x) <= TOL:
                    keep.append(c)
        cands = keep
        print(f"round {r} at ({x:.0f}, {y:.0f}): {len(cands)} candidates", flush=True)
    x, y = player(status)
    for c in cands[:40]:
        if MODE != "pair":
            try:
                raw = os.pread(mem, 64, c - 16)
            except OSError:
                continue
            f8 = " ".join(f"{v:.2f}" for v in np.frombuffer(raw, dtype="<f8"))
            f4 = " ".join(f"{v:.2f}" for v in np.frombuffer(raw, dtype="<f4"))
            print(f"  {c:#x} x={read_one(mem, c):.3f}\n     f64 around: {f8}\n     f32 around: {f4}   (stream {x:.0f},{y:.0f})")
            continue
        try:
            raw = os.pread(mem, 48, c)
        except OSError:
            continue
        d = np.frombuffer(raw, dtype="<f8")
        print(f"  {c:#x}  " + " ".join(f"{v:10.3f}" for v in d) + f"   (stream {x:.0f},{y:.0f})")


if __name__ == "__main__":
    main()
