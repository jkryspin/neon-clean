#!/usr/bin/env python3
"""Append opcodes seen in logs but missing from neon-protocol/opcodes.toml as `observed` entries.

  scripts/registry-merge.py            (reads run/opcodes/*.json for incoming, docs/outgoing-opcodes.json for outgoing)

Never edits existing entries: names, statuses and field layouts are only added by hand, with evidence.
"""
import glob, json, re, sys

REG = "neon-protocol/opcodes.toml"
try:
    text = open(REG).read()
except FileNotFoundError:
    text = "# Opcode registry. status: verified (fields tested on real packets) | decoded (decoded in neon-stream, layout not here yet) | structure | observed\n"
have = set()
for m in re.finditer(r'\[\[opcode\]\]\nid = (0x[0-9a-fA-F]+)\ndir = "(in|out)"', text):
    have.add((m[2], int(m[1], 16)))
add = []
inc = {}
for p in sorted(glob.glob("run/opcodes/*.json")):
    for k, v in json.load(open(p)).items():
        a = inc.setdefault(int(k, 16), [10**9, 0])
        a[0] = min(a[0], v["min_len"]); a[1] = max(a[1], v["max_len"])
for op, (lo, hi) in sorted(inc.items()):
    if ("in", op) not in have:
        add.append(("in", op, lo, hi))
try:
    og = json.load(open("docs/outgoing-opcodes.json"))["opcodes"]
except OSError:
    og = {}
for k in sorted(og):
    op = int(k, 16)
    if ("out", op) not in have:
        lens = [int(x) - 4 for x in og[k]["next_send_wire_lens"]]
        add.append(("out", op, None, None))
for d, op, lo, hi in add:
    text += f'\n[[opcode]]\nid = 0x{op:04x}\ndir = "{d}"\nstatus = "observed"\n'
    if lo is not None:
        text += f"len_min = {lo}\nlen_max = {hi}\n"
open(REG, "w").write(text)
print(f"added {len(add)} observed opcodes to {REG}")
