#!/usr/bin/env bash
# Read code out of a live MapleStory client WITHOUT attaching a debugger and disassemble it offline.
#
#   neon-calib/tools/exeread.sh ADDR [LEN] [PID]
#
# ADDR is the absolute address as it appears in docs/network-protocol.md (e.g. 0x1416667a0), LEN defaults to 512
# bytes, PID defaults to the jib01 client (never default to the main account).
#
# Why this way: attaching a debugger (gdb) to this client has twice dropped BOTH test accounts' sessions to the
# login screen within 60-150 s - see docs/network-protocol.md's risk note and the gdb-attach-drops-sessions
# memory. Plain reads of /proc/PID/mem use no ptrace, stop no threads, and have been used safely all through the
# cipher work; keep it that way. This script must never grow a gdb/ptrace path.
#
# Addresses are build-specific (Classic World v1.0.1). Re-verify after any client update.
set -euo pipefail

ADDR="${1:?usage: exeread.sh ADDR [LEN] [PID]}"
LEN="${2:-512}"
PID="${3:-$(pgrep -f 'NexonLauncher-jib01.*MapleStory.exe' | head -1)}"
[ -n "$PID" ] || { echo "no jib01 MapleStory.exe found; pass a PID explicitly" >&2; exit 1; }

ADDR_DEC=$((ADDR))
OUT=$(mktemp /tmp/exeread.XXXXXX.bin)
trap 'rm -f "$OUT"' EXIT

# dd on /proc/PID/mem: seek to the absolute virtual address and read LEN bytes. No ptrace, no stopping threads.
dd if="/proc/$PID/mem" of="$OUT" bs=1 skip="$ADDR_DEC" count="$LEN" status=none 2>/dev/null || {
    echo "read failed at $ADDR (pid $PID) - address not mapped, or process gone" >&2; exit 1; }

objdump -D -b binary -mi386:x86-64 -M intel --adjust-vma="$ADDR" "$OUT" | tail -n +7
