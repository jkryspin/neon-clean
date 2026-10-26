#!/usr/bin/env bash
# Log outgoing opcodes of a running client (reads the packet object before encryption; passive, writes nothing).
#   sudo-capable user:  neon-calib/tools/outcap.sh <game pid> <seconds> [out.json]
# Addresses are for Classic World v1.0.1 (see docs/network-protocol.md); re-find them if the build changes.
set -eu
PID=$1; SECS=$2; export OUTCAP_JSON=${3:-outcap.json}
HERE=$(cd "$(dirname "$0")" && pwd)
