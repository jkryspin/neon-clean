#!/usr/bin/env python3
"""Action lab: find which incoming opcodes one in-game action produces, with a baseline and repeats.

  actlab.py --display 101 --log in.log --actions actions.json [--baseline 40] [--repeat 3] [--settle 6] [--out report.json]

in.log is the timestamped dump written by (see docs/opcodes.md, "capture"):
  NEON_DUMP_OPS=all neon-stream --pid PID --chains neon-calib/chains.json --net --no-footholds 2> >(python3 stamp.py in.log)
Each line: "<unix time> op <hex4> len <n>: <hex bytes>".

actions.json: [{"name": "party_create", "pre": [...setup, untimed], "steps": [...timed], "undo": [...]}]
steps: ["key","k"] | ["click",x,y] | ["type","text"] | ["sleep",seconds]; coordinates are X root coordinates.

Method: idle baseline first; then each action `repeat` times (undo steps run after the settle window). An opcode is a
CANDIDATE for an action only if it is absent from the whole baseline and appears in *every* repeat of that action
within the settle window. Candidates still need a second look (payload) before they get a name in the registry.
Only drives the keyboard/mouse of the given X display; never touches the game process.
"""
import argparse, collections, json, os, re, subprocess, sys, time


def parse_log(path):
    pk = []
    for line in open(path, errors="replace"):
        m = re.match(r"(\S+) op (\w+) len (\d+): ?(.*)", line)
        if m:
            pk.append((float(m[1]), m[2], int(m[3]), m[4].strip()))
    return pk


def run_steps(env, steps):
    for st in steps:
        k = st[0]
        if k == "key":
            subprocess.run(["xdotool", "key", st[1]], env=env, check=False)
        elif k == "click":
            subprocess.run(["xdotool", "mousemove", str(st[1]), str(st[2]), "click", "1"], env=env, check=False)
        elif k == "type":
            subprocess.run(["xdotool", "type", "--delay", "80", st[1]], env=env, check=False)
        elif k == "sleep":
            time.sleep(float(st[1]))
        else:
            raise SystemExit(f"unknown step {st}")
        time.sleep(0.25)


def candidates(pk, baseline, runs):
    """runs: list of (t0, t1) windows of one action. Returns opcodes in every window and not in the baseline."""
    base_ops = {op for (t, op, n, h) in pk if baseline[0] <= t < baseline[1]}
    per_run = []
    for t0, t1 in runs:
        per_run.append(collections.Counter(op for (t, op, n, h) in pk if t0 <= t < t1))
    common = set(per_run[0]).intersection(*[set(c) for c in per_run[1:]]) if per_run else set()
    return sorted(common - base_ops), base_ops, per_run


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--display", required=True)
    ap.add_argument("--log", required=True)
    ap.add_argument("--actions", required=True)
    ap.add_argument("--baseline", type=float, default=40)
    ap.add_argument("--repeat", type=int, default=3)
    ap.add_argument("--settle", type=float, default=6)
    ap.add_argument("--out", default="actlab-report.json")
    a = ap.parse_args()
    env = dict(os.environ, DISPLAY=f":{a.display}", XAUTHORITY="")
    actions = json.load(open(a.actions))
    t_b0 = time.time()
    print(f"baseline {a.baseline:.0f}s ...", flush=True)
    time.sleep(a.baseline)
    baseline = (t_b0, time.time())
    windows = {}
    for act in actions:
        for i in range(a.repeat):
            run_steps(env, act.get("pre", []))
            time.sleep(a.settle)  # let the setup traffic die down before the timed window
            t0 = time.time()
            run_steps(env, act["steps"])
            time.sleep(a.settle)
            t1 = time.time()
            windows.setdefault(act["name"], []).append((t0, t1))
            run_steps(env, act.get("undo", []))
            time.sleep(a.settle)  # quiet gap so the next repeat is not contaminated
            print(f"  {act['name']} #{i + 1} done", flush=True)
    time.sleep(2)
    pk = parse_log(a.log)
    report = {"baseline_s": baseline[1] - baseline[0], "baseline_ops": {}, "actions": {}}
    for act in actions:
        cand, base_ops, per_run = candidates(pk, baseline, windows[act["name"]])
        report["baseline_ops"] = sorted(base_ops)
        info = {}
        for op in cand:
            sizes = sorted({n for (t, o, n, h) in pk if o == op and any(w0 <= t < w1 for w0, w1 in windows[act["name"]])})
            sample = next(h for (t, o, n, h) in pk if o == op and any(w0 <= t < w1 for w0, w1 in windows[act["name"]]))
            info[op] = {"counts_per_run": [c[op] for c in per_run], "sizes": sizes, "sample": sample[:96]}
        report["actions"][act["name"]] = {"candidates": info, "runs": len(windows[act["name"]])}
        print(f"{act['name']}: candidates {sorted(info) or 'none'}")
    json.dump(report, open(a.out, "w"), indent=1)
    print("wrote", a.out)


if __name__ == "__main__":
    main()
