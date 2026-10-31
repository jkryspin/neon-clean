#!/usr/bin/env python3
"""Headless control for neon-hack.dll (the panel does the same over the same mapping).

    python hackctl.py <pid> status
    python hackctl.py <pid> on|off
    python hackctl.py <pid> force <hexaddr> <value>
"""

import ctypes
import ctypes.wintypes as w
import sys
import time

sys.path.insert(0, str(__import__("pathlib").Path(__file__).resolve().parent))
from panel import NEON_HACK_STATE, MAGIC, k32, STATES  # noqa: E402


def open_state(pid):
    name = f"Local\\NeonHack.{pid}"
    m = k32.OpenFileMappingW(0x0006, False, name)
    if not m:
        sys.exit(f"no mapping {name} (not attached)")
    v = k32.MapViewOfFile(m, 0x0006, 0, 0, 0)
    st = NEON_HACK_STATE.from_address(v)
    if st.magic != MAGIC:
        sys.exit("bad magic")
    return st


def main():
    pid = int(sys.argv[1])
    action = sys.argv[2] if len(sys.argv) > 2 else "status"
    st = open_state(pid)
    if action == "on":
        st.cmd = 1
    elif action == "off":
        st.cmd = 0
    elif action == "force":
        st.force_addr = int(sys.argv[3], 16)
        st.force_value = int(sys.argv[4], 0)
        st.cmd = 2
    for _ in range(40):
        print(f"cmd={st.cmd} state={STATES.get(st.state)} cand={st.candidates} "
              f"dmg={st.damage_events} hp={st.hp}/{st.max_hp} frozen={st.frozen_count} "
              f"ticks={st.ticks}")
        if st.frozen_count:
            print("frozen:", [hex(st.frozen_addr[i]) for i in range(st.frozen_count)])
        if action == "status":
            break
        time.sleep(0.5)


if __name__ == "__main__":
    main()
