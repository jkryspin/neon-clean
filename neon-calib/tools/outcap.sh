#!/usr/bin/env bash
# Log outgoing opcodes of a running client (reads the packet object before encryption; passive, writes nothing).
#   neon-calib/tools/outcap.sh <game pid> <seconds> [out.json]
# Optional: OUTCAP_EVENTS=file.jsonl also logs every packet with a timestamp (holds payload bytes: keep it local).
# Needs passwordless `sudo gdb`. Addresses are for Classic World v1.0.1 (docs/network-protocol.md).
# Note: the client is paused for every breakpoint; keep captures short or the server may drop the connection.
set -eu
PID=$1; SECS=$2; export OUTCAP_JSON=${3:-outcap.json}
HERE=$(cd "$(dirname "$0")" && pwd)
G=$(mktemp --suffix=.gdb)
cat >"$G" <<GDB
set architecture i386:x86-64
set pagination off
set confirm off
handle SIGUSR1 nostop noprint pass
handle SIGUSR2 nostop noprint pass
handle SIGALRM nostop noprint pass
handle SIGCHLD nostop noprint pass
handle SIGSEGV nostop noprint pass
handle SIG34 nostop noprint pass
handle SIG35 nostop noprint pass
handle SIGPWR nostop noprint pass
handle SIGXCPU nostop noprint pass
source $HERE/outcap.py
continue
GDB
timeout "$SECS" sudo -n gdb -p "$PID" -batch -x "$G" >"${OUTCAP_JSON}.gdb.log" 2>&1 || true
rm -f "$G"
echo "wrote $OUTCAP_JSON"
