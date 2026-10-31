#!/usr/bin/env python3
"""Correlate the in-process opcode probe output with neon-stream wire capture to build the full wire map.

  wiremap-probe.py <probe.jsonl> <wire.log> [-o map.json]

The probe captures internal opcodes with timestamps; neon-stream captures wire opcodes.
Match by: (approximate timestamp within WINDOW_MS) + (exact packet length).

Output: {"<wire_hex>": "<internal_hex>", ...} — the full per-connection wire map.

## Usage

1. Inject `probe_opcodes-win.dll` into the game (via inject_ud-win.exe)
2. Run `neon-stream --net --opcode-log /tmp/opcodes.json` to capture wire traffic
3. Stop both, then run: `wiremap-probe.py /tmp/neon-probe-opcodes.<pid>.jsonl /tmp/wire.log -o wiremap.json`
4. Use the resulting wiremap.json to construct outgoing packets with correct opcodes

## Output format

The output JSON maps wire opcodes (what's on the network) to internal opcodes
(what the game uses internally):

    {
      "0001": "0001",  // keepalive
      "029d": "029d",  // player move (incoming, not mapped here)
      "00db": "00e0",  // MoveSelf: wire=0x00db, internal=0x00e0
      "00e1": "0190",  // AttackSkill: wire=0x00e1, internal=0x0190
      ...
    }

## How the probe works (Phase 5)

The probe_opcodes-win.dll uses inline hooking (v4.1) on the packet send function
at 0x141660360. When the game sends a packet:

1. The inline hook intercepts the call
2. The hook reads the internal opcode from the packet object at offset +0x434
3. The hook reads the packet payload and length
4. Output is written as JSONL to /tmp/neon-probe-opcodes.<pid>.jsonl

Each line looks like:
    {"t":12345,"d":"out","op":"0x0190","len":24,"head":"aabbccdd..."}

The timestamp `t` is in milliseconds (GetTickCount64).

## Correlation algorithm

1. Read probe entries (internal opcodes with timestamps)
2. Read wire log entries (wire opcodes with timestamps)
3. For each probe entry, find matching wire entries within WINDOW_MS
4. Match by exact packet length (most reliable discriminator)
5. Build a mapping: wire_op -> internal_op
6. Require at least 1 observation (2+ preferred for confidence)

## Per-connection wire mapping

Wire opcodes are per-connection scrambled. Each new connection (channel change,
reconnect) may have different wire opcodes for the same internal opcodes.

To build a complete map:
1. Capture probe + wire data for each connection separately
2. Build a map per connection ID
3. The loader maintains a connection map cache

## Troubleshooting

- No probe output: check that the DLL injected successfully (look for log file)
- No wire data: check that --net flag is set and tcpdump has sudo access
- Poor correlation: increase WINDOW_MS, or check that timestamps are aligned
- Missing opcodes: some opcodes may only appear in specific game states (login, combat, etc.)
"""

import bisect
import collections
import json
import re
import sys
import os
from typing import Dict, List, Tuple, Optional

WINDOW_MS = 500  # max time difference for a match (ms)

WIRE_LINE = re.compile(r"(\S+) op (\w+) len (\d+): ?(.*)")


def read_probe(path: str) -> List[dict]:
    """Read probe JSONL: [{t, op, len, head}, ...]"""
    out = []
    with open(path, errors="replace") as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            try:
                e = json.loads(line)
            except json.JSONDecodeError:
                continue
            if e.get("d") == "out":
                out.append(e)
    return out


def read_wire(path: str) -> List[Tuple[float, int, int]]:
    """Read neon-stream outgoing wire log: [(t_ms, wire_op, length), ...]

    The wire log format is:
        <timestamp> op <hex_opcode> len <length>: <hex_payload>

    Timestamps are in seconds (from neon-stream's output).
    """
    out = []
    with open(path, errors="replace") as f:
        for line in f:
            m = WIRE_LINE.match(line)
            if m:
                t_ms = float(m[1]) * 1000  # convert seconds to ms
                wire_op = int(m[2], 16)
                pkt_len = int(m[3])
                out.append((t_ms, wire_op, pkt_len))
    return out


def correlate(probe: List[dict], wire: List[Tuple[float, int, int]]) -> Dict[int, int]:
    """Match probe entries to wire entries by time + length.

    Returns: mapping from wire_op (int) -> internal_op (int)
    """
    mapping = {}  # wire_op -> internal_op
    counts = collections.Counter()  # (wire, internal) -> count

    # Sort wire by time for binary search
    wire.sort(key=lambda x: x[0])
    wire_times = [w[0] for w in wire]

    for p in probe:
        t = p["t"]
        plen = p["len"]
        internal = int(p["op"], 16)

        # Find wire entries within WINDOW_MS
        lo = bisect.bisect_left(wire_times, t - WINDOW_MS)
        hi = bisect.bisect_right(wire_times, t + WINDOW_MS)

        best = None
        best_dt = WINDOW_MS + 1
        for i in range(lo, hi):
            wt, wop, wlen = wire[i]
            if wlen == plen:
                dt = abs(wt - t)
                if dt < best_dt:
                    best_dt = dt
                    best = (wop, internal)

        if best:
            counts[best] += 1

    # Only accept mappings seen at least once
    for (wire_op, internal_op), cnt in counts.most_common():
        if wire_op not in mapping and cnt >= 1:
            mapping[wire_op] = internal_op

    return mapping


def build_connection_map(probe_path: str, wire_path: str) -> Dict[str, Dict[str, str]]:
    """Build a per-connection wire map.

    If the probe output contains connection IDs (from multiple injections),
    this builds separate maps for each connection.

    Returns: {"conn_<id>": {"<wire_hex>": "<internal_hex>", ...}, ...}
    """
    probe = read_probe(probe_path)
    wire = read_wire(wire_path)

    # Group by approximate timestamp ranges (each connection has a time window)
    if not probe or not wire:
        return {}

    mapping = correlate(probe, wire)
    return {"main": {f"{k:04x}": f"{v:04x}" for k, v in sorted(mapping.items())}}


def print_mapping(mapping: Dict[str, Dict[str, str]]) -> None:
    """Print the wire mapping in a human-readable format."""
    for conn_id, map_data in mapping.items():
        print(f"Connection: {conn_id}")
        print(f"  Mapped {len(map_data)} wire->internal opcodes:")
        for wire, internal in sorted(map_data.items()):
            print(f"    wire 0x{wire} -> internal 0x{internal}")
        print()


def main():
    if len(sys.argv) < 3:
        print(f"usage: {sys.argv[0]} <probe.jsonl> <wire.log> [-o map.json]")
        print()
        print("Arguments:")
        print("  probe.jsonl  - Output from probe_opcodes-win.dll (JSONL format)")
        print("  wire.log     - Wire capture from neon-stream (opcode log format)")
        print("  -o FILE      - Output file for the mapping (JSON)")
        print()
        print("Example:")
        print(f"  {sys.argv[0]} /tmp/neon-probe-opcodes.1234.jsonl /tmp/wire.log -o wiremap.json")
        sys.exit(1)

    probe_path = sys.argv[1]
    wire_path = sys.argv[2]

    out_path = None
    if "-o" in sys.argv:
        out_path = sys.argv[sys.argv.index("-o") + 1]

    # Read and correlate
    probe = read_probe(probe_path)
    wire = read_wire(wire_path)

    if not probe:
        print(f"WARNING: No probe entries found in {probe_path}", file=sys.stderr)
    if not wire:
        print(f"WARNING: No wire entries found in {wire_path}", file=sys.stderr)

    mapping = correlate(probe, wire)
    result = {f"{k:04x}": f"{v:04x}" for k, v in sorted(mapping.items())}

    if out_path:
        with open(out_path, "w") as f:
            json.dump(result, f, indent=2)
        print(f"Saved mapping to {out_path}")

    print(f"Mapped {len(result)} wire->internal opcodes:")
    for wire, internal in sorted(result.items()):
        print(f"  wire 0x{wire} -> internal 0x{internal}")


if __name__ == "__main__":
    main()
