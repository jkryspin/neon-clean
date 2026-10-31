#!/usr/bin/env python3
"""Autonomous i-frame hunter.

Watches the HP field in memory; when it drops (a hit landed), it re-seeds a small scan
window around that field to zeros and keeps only addresses that rise and then fall back
- the shape of an invincibility/damage-cooldown counter.

    python neon-win/iframes.py --hp 6279578 --region 6278D78+1000 --hits 6

Survivors are printed with a hit count; anything appearing on several rounds is a
candidate to freeze.
"""

import argparse
import collections
import json
import os
import sys
import time
import urllib.request
from pathlib import Path

SESSION = Path(os.environ.get("LOCALAPPDATA", str(Path.home()))) / "neon" / "ce" / "session.json"


class Api:
    def __init__(self):
        s = json.loads(SESSION.read_text())
        self.port, self.token = s["port"], s["token"]

    def call(self, method, **params):
        body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
        req = urllib.request.Request(
            f"http://127.0.0.1:{self.port}/rpc", data=body,
            headers={"Authorization": f"Bearer {self.token}", "Content-Type": "application/json"})
        with urllib.request.urlopen(req, timeout=30) as r:
            out = json.loads(r.read())
        if "error" in out:
            raise RuntimeError(out["error"].get("message"))
        return out.get("result")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--hp", required=True, help="hex address of the HP field")
    ap.add_argument("--region", required=True, help="BASE+SIZE hex window to scan")
    ap.add_argument("--type", default="4 bytes")
    ap.add_argument("--hits", type=int, default=6, help="stop after this many hits")
    ap.add_argument("--seconds", type=int, default=300)
    ap.add_argument("--settle", type=float, default=0.25, help="seconds before the rise pass")
    ap.add_argument("--fall", type=float, default=0.4, help="seconds before the fall pass")
    args = ap.parse_args()

    api = Api()
    hp_addr = args.hp
    session = api.call("scan_create", type=args.type, scope="range:" + args.region,
                       alignment=4, cmp="eq", value="0")["session"]
    print(f"session {session}", flush=True)

    last = api.call("read", addr=hp_addr, type="4 bytes")["value"]
    last = int(last)
    print(f"hp start {last}", flush=True)
    hits = 0
    seen = collections.Counter()
    start = time.time()

    while hits < args.hits and time.time() - start < args.seconds:
        time.sleep(0.03)
        try:
            cur = int(api.call("read", addr=hp_addr, type="4 bytes")["value"])
        except Exception:
            time.sleep(0.2)
            continue
        if cur >= last:
            last = cur
            continue
        hits += 1
        print(f"hit {hits}: hp {last} -> {cur}", flush=True)
        last = cur
        api.call("scan_reseed", session=session, cmp="eq", value="0")
        time.sleep(args.settle)
        try:
            n1 = api.call("scan_next", session=session, cmp="changed")["left"]
            time.sleep(args.fall)
            n2 = api.call("scan_next", session=session, cmp="decreased")["left"]
        except Exception as e:
            print(f"  pass error: {e}", flush=True)
            continue
        print(f"  changed={n1} fell={n2}", flush=True)
        if 0 < n2 <= 400:
            rows = api.call("scan_results", session=session, limit=400)["rows"]
            for row in rows:
                seen[row["addr_hex"]] += 1
            top = seen.most_common(5)
            print("  top: " + ", ".join(f"{a} x{n}" for a, n in top), flush=True)

    print("\nfinal candidates:")
    for addr, n in seen.most_common(25):
        print(f"  {addr}  x{n}")
    api.call("scan_drop", session=session)


if __name__ == "__main__":
    main()
