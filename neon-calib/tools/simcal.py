#!/usr/bin/env python3
"""Controlled movement recording for the shadow simulation (run it in a town: no mobs).

Starts neon-stream, presses a fixed sequence of moves (walks, standing jumps, running jumps,
air control) with xdotool and writes every stream frame as a neon-loader decision-trace line:
  <ts> pos=x,y gr=Some(true) tgt=- mobs=0 | cal <step> | <actions sent since the last frame>
Then compare and fit with:
  neon-loader --maps-dir neon-calib/maps --sim-replay OUT.trace MAP --sim-log OUT.sim
  python3 neon-calib/tools/simfit.py OUT.sim

usage: simcal.py OUT.trace [--reps N]
Moves are read-only for the game except the key presses themselves.
"""
import json
import os
import subprocess
import sys
import threading
import time

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
JUMP = "alt"


def game_pid():
    out = subprocess.run(["pgrep", "-f", "MapleStory.exe"], capture_output=True, text=True).stdout.split()
    for p in out:
        try:
            cmd = open(f"/proc/{p}/cmdline", "rb").read().replace(b"\0", b" ").decode(errors="replace")
        except OSError:
            continue
        if cmd.startswith("C:\\") and "MapleStory.exe" in cmd:
            return p
    sys.exit("game not running")


def x_env():
    env = dict(os.environ)
    env.setdefault("DISPLAY", ":0")
    if "XAUTHORITY" not in env:
        d = f"/run/user/{os.getuid()}"
        for f in os.listdir(d):
            if f.startswith(".mutter-Xwaylandauth."):
                env["XAUTHORITY"] = os.path.join(d, f)
    return env


ENV = x_env()
pending = []
lock = threading.Lock()


def xdo(*args):
    subprocess.run(["xdotool", *args], env=ENV, check=False)


def act(label, *args):
    # Stamp the press itself: the game reacts within a frame, so labelling the next frame would put the
    # input after its own effect.
    t = int(time.time() * 1000)
    xdo(*args)
    with lock:
        pending.append((t, label))


def down(d):
    act(f"Move(Some({d}))", "keydown", d)


def up(d):
    act("Move(None)", "keyup", d)


def jump():
    act(f'Tap("{JUMP}")', "key", JUMP)


def sequence(reps, step):
    s = time.sleep
    step[0] = "idle"
    s(1.0)
    for _ in range(reps):
        for d in ("Right", "Left"):
            step[0] = f"walk {d}"
            down(d); s(1.5); up(d); s(0.7)
        step[0] = "standing jumps"
        for _ in range(2):
            jump(); s(1.3)
        for d in ("Right", "Left"):
            step[0] = f"running jump {d}"
            down(d); s(0.6); jump(); s(0.9); up(d); s(0.8)
        for d in ("Left", "Right"):
            step[0] = f"air control {d}"
            jump(); s(0.12); down(d); s(0.6); up(d); s(0.8)
        for d in ("Right", "Left"):
            step[0] = f"direction+jump together {d}"
            down(d); jump(); s(0.9); up(d); s(0.8)
        for d in ("Right", "Left"):
            for run in (0.08, 0.15, 0.3):
                step[0] = f"run-up {run}s jump {d}"
                down(d); s(run); jump(); s(0.9); up(d); s(0.8)
        for d in ("Right", "Left"):
            step[0] = f"jump then release {d}"
            down(d); s(0.5); jump(); s(0.1); up(d); s(1.0)
        for d in ("Right", "Left"):
            step[0] = f"tap walk {d}"
            for ms in (60, 120, 250):
                down(d); s(ms / 1000); up(d); s(0.5)
    step[0] = "done"


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    out = sys.argv[1]
    reps = int(sys.argv[sys.argv.index("--reps") + 1]) if "--reps" in sys.argv else 2
    stream = subprocess.Popen(
        ["nice", "-n", "15", f"{ROOT}/neon-stream/target/release/neon-stream", "--pid", game_pid(),
         "--chains", f"{ROOT}/neon-calib/chains.json", "--no-footholds"],
        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
    # Keys only reach the game after a click inside it (sky: nothing to hit).
    wid = subprocess.run(["xdotool", "search", "--name", "MapleStory"], env=ENV, capture_output=True, text=True).stdout.split()[0]
    xdo("mousemove", "--window", wid, "700", "125", "click", "1")
    step = ["start"]
    t = threading.Thread(target=sequence, args=(reps, step), daemon=True)
    started = False
    frames = 0
    last = None
    try:
        with open(out, "w") as f:
            for line in stream.stdout:
                try:
                    st = json.loads(line)
                except ValueError:
                    continue
                if not started:
                    t.start()
                    started = True
                p = st.get("player", {})
                gr = p.get("grounded")
                grs = "None" if gr is None else f"Some({str(gr).lower()})"
                with lock:
                    acts = list(pending)
                    pending.clear()
                # Each press is its own line at its own time, with the last known position (the replay advances
                # the shadow to that instant and applies the input from there).
                for t, label in acts:
                    if last and t > last[0]:
                        f.write(f"{t} pos={last[1]} gr={last[2]} tgt=- mobs=0 | cal {step[0]} | {label}\n")
                    else:
                        f.write(f"{st.get('ts_ms')} pos={p.get('x')},{p.get('y')} gr={grs} tgt=- mobs=0 | cal {step[0]} | {label}\n")
                f.write(f"{st.get('ts_ms')} pos={p.get('x')},{p.get('y')} gr={grs} tgt=- mobs=0 | cal {step[0]} | \n")
                last = (st.get("ts_ms"), f"{p.get('x')},{p.get('y')}", grs)
                frames += 1
                if step[0] == "done":
                    break
    finally:
        for k in ("Left", "Right", JUMP):
            xdo("keyup", k)
        stream.terminate()
    print(f"{frames} frames -> {out}")


if __name__ == "__main__":
    main()
