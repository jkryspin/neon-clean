#!/usr/bin/env bash
# Short, guarded capture that pairs the client's internal packet object with the wire plaintext handed to the cipher (see neon-calib/tools/outwire.py).
#
#   scripts/capture-wire.sh NAME SECONDS OUT.jsonl [--allow-main]
#
# Guard rails, learned on 2026-10-26 (2 of 5 attaches were followed by a delayed disconnect):
#   * refuses the main account (registry entry with a "prefix") unless --allow-main is given;
#   * at most 20 s of attach (the capture itself is shorter: gdb needs a few seconds to attach and detach);
#   * at least 5 minutes since the previous attach of any account (run/attach-log.txt);
#   * every attach is appended to run/attach-log.txt with its time and account;
#   * afterwards it waits and prints whether the view still sees the character.
# Needs passwordless `sudo gdb`. The output holds plaintext payloads: keep it local (*.nsr is git-ignored).
set -eu
NAME=${1:?usage: capture-wire.sh NAME SECONDS OUT.jsonl [--allow-main]}
SECS=${2:?seconds}
OUT=${3:?output .nsr}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
ALLOW_MAIN=0; [ "${4:-}" = "--allow-main" ] && ALLOW_MAIN=1
[ "$SECS" -le 20 ] || { echo "refusing: $SECS s is longer than the 20 s cap"; exit 2; }
MAIN=$(python3 -c "import json,sys; print(1 if 'prefix' in json.load(open('neon-instances.json'))[sys.argv[1]] else 0)" "$NAME") || exit 2
if [ "$MAIN" = 1 ] && [ "$ALLOW_MAIN" = 0 ]; then
  echo "refusing: $NAME is the main account (it has a registry prefix). Use a test account, or --allow-main if you really mean it."; exit 2
fi
mkdir -p run
LOG=run/attach-log.txt
if [ -f "$LOG" ]; then
  LAST=$(tail -1 "$LOG" | awk '{print $1}')
  NOW=$(date +%s)
  if [ -n "$LAST" ] && [ $((NOW - LAST)) -lt 300 ]; then
    echo "refusing: the last attach was $((NOW - LAST)) s ago (minimum 300 s)"; exit 2
  fi
fi
PFXNAME=$(python3 -c "import json,sys; print(json.load(open('neon-instances.json'))[sys.argv[1]].get('prefix','NexonLauncher-'+sys.argv[1]))" "$NAME")
PFX=$HOME/.var/app/io.github.fastrizwaan.WineCharm/data/winecharm/Prefixes/$PFXNAME
PID=
for p in $(pgrep -f MapleStory.exe); do
  tr '\0' ' ' </proc/$p/cmdline 2>/dev/null | grep -q '^C:' && grep -qz "^WINEPREFIX=$PFX$" /proc/$p/environ 2>/dev/null && { PID=$p; break; }
done
[ -n "$PID" ] || { echo "$NAME: game not running"; exit 3; }
RAW=$(mktemp --suffix=.jsonl); chmod 666 "$RAW"
G=$(mktemp --suffix=.gdb)
cat >"$G" <<GDB
set architecture i386:x86-64
set pagination off
set confirm off
handle SIGUSR1 nostop noprint pass
handle SIGSTOP nostop noprint nopass
handle SIGUSR2 nostop noprint pass
handle SIGALRM nostop noprint pass
handle SIGCHLD nostop noprint pass
handle SIGSEGV nostop noprint pass
handle SIG34 nostop noprint pass
handle SIG35 nostop noprint pass
handle SIGPWR nostop noprint pass
handle SIGXCPU nostop noprint pass
source $ROOT/neon-calib/tools/outwire.py
python
import time
_end = time.time() + $SECS
while time.time() < _end:
    try:
        gdb.execute('continue')
    except Exception:
        break
end
GDB
echo "$(date +%s) $NAME pid=$PID secs=$SECS -> $OUT" >> "$LOG"
echo "attaching to $NAME (pid $PID) for $SECS s ... do the action now (it takes ~5 s to attach)"
OUTWIRE_JSONL=$RAW timeout "$SECS" sudo -n --preserve-env=OUTWIRE_JSONL gdb -p "$PID" -batch -x "$G" >"$OUT.gdblog" 2>&1 || true
cp "$RAW" "$OUT"
rm -f "$RAW" "$G"
echo "done. Watch the account for a few minutes: the disconnects came 3-8 minutes after a capture."
