#!/usr/bin/env python3
"""Which opcodes does a hand-made action produce? Line up labelled marks with the packets after them.

  marklab.py <marks.jsonl> <packet log> [<more logs>...] [--window 15]

Marks are what you type into Neon View while you do something in the game ("opened the shop"); each is {"t": unix ms, "label": ...}.
For every mark the window is [t, min(next mark, t + window s)). An opcode (per direction) is a candidate for a mark when it occurs in the
window and EITHER never occurs outside all windows... no: when its count in the window is at least 5x what the rate outside every window predicts
(and at least 2 packets), or it occurs nowhere else at all. Needs precise packet times: use logs written since the stream began
carrying capture times (older logs are quantised to ~1 s). Evidence only: repeat the action and mark it again; an opcode that shows up
after every repeat is the one to look at. Reads the same formats as neon-view (.nsr JSON lines, stamped logs).
"""
import collections, json, re, sys

def load(paths):
    """A path ending in .wire holds client->server packets (decrypted from the wire); every other text log is server->client."""
    recs = []
    for path in paths:
        text_dir = "out" if path.endswith(".wire") else "in"
        for line in open(path, errors="replace"):
            line = line.strip()
            if line.startswith("{"):
                try:
                    j = json.loads(line)
                except ValueError:
                    continue
                b = bytes.fromhex(j["hex"])
                if len(b) >= 2:
                    recs.append((j["t"] / 1000.0, j["d"], b[0] | b[1] << 8, len(b)))
            else:
                m = re.match(r"(\S+) op (\w+) len (\d+):", line)
                if m:
                    recs.append((float(m[1]), text_dir, int(m[2], 16), int(m[3])))
    recs.sort()
    return recs

def analyse(marks, recs, window=15.0, factor=5.0):
    """Returns [(label, t, [(dir, op, count_in_window, expected, sizes)])]."""
    if not marks or not recs:
        return []
    marks = sorted(marks, key=lambda m: m["t"])
    wins = []
    for i, m in enumerate(marks):
        t0 = m["t"] / 1000.0
        t1 = min(marks[i + 1]["t"] / 1000.0 if i + 1 < len(marks) else 1e18, t0 + window)
        wins.append((t0, t1))
    def inside(t):
        return any(a <= t < b for a, b in wins)
    span_total = recs[-1][0] - recs[0][0]
    span_win = sum(min(b, recs[-1][0]) - max(a, recs[0][0]) for a, b in wins if b > recs[0][0] and a < recs[-1][0])
    span_out = max(1.0, span_total - span_win)
    outside = collections.Counter((d, op) for t, d, op, n in recs if not inside(t))
    out = []
    for (t0, t1), m in zip(wins, marks):
        c = collections.Counter(); sizes = collections.defaultdict(collections.Counter)
        for t, d, op, n in recs:
            if t0 <= t < t1:
                c[(d, op)] += 1; sizes[(d, op)][n] += 1
        cands = []
        for (d, op), k in c.items():
            expected = outside[(d, op)] / span_out * (t1 - t0)
            if k >= 2 and k >= factor * max(expected, 0.2) or (outside[(d, op)] == 0 and k >= 1):
                cands.append((d, op, k, round(expected, 2), dict(sizes[(d, op)].most_common(3))))
        cands.sort(key=lambda x: -x[2] / max(x[3], 0.2))
        out.append((m["label"], m["t"], cands))
    return out

if __name__ == "__main__":
    args = sys.argv[1:]
    window = 15.0
    if "--window" in args:
        i = args.index("--window"); window = float(args[i + 1]); del args[i:i + 2]
    marks = [json.loads(l) for l in open(args[0]) if l.strip()]
    for label, t, cands in analyse(marks, load(args[1:]), window):
        print(f"\n[{label}]  ({len(cands)} candidate opcodes)")
        for d, op, k, e, sz in cands[:12]:
            print(f"   {'->' if d == 'out' else '<-'} 0x{op:04x}: {k} in the window, {e} expected; sizes {sz}")
