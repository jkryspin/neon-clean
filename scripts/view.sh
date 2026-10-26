#!/usr/bin/env bash
# Live packet view for one instance: neon-stream (passive, tcpdump) -> stamped packet log -> neon-view on 0.0.0.0:PORT.
#
#   scripts/view.sh NAME [PORT]      NAME is a key of neon-instances.json (default port 8765)
#   scripts/view.sh stop NAME
#
# Read-only: it never sends keys or packets. The page has no authentication (LAN only). Needs `sudo -n tcpdump`.
set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
if [ "${1:-}" = stop ]; then
  NAME=${2:?usage: view.sh stop NAME}
  [ -f "run/view.$NAME.pgid" ] && kill -TERM -- "-$(cat "run/view.$NAME.pgid")" 2>/dev/null
  rm -f "run/view.$NAME.pgid"; echo "$NAME: view stopped"; exit 0
fi
NAME=${1:?usage: view.sh NAME [PORT]}
PORT=${2:-8765}
PFXNAME=$(python3 -c "import json,sys; print(json.load(open('neon-instances.json'))[sys.argv[1]].get('prefix','NexonLauncher-'+sys.argv[1]))" "$NAME") || exit 2
PFX=$HOME/.var/app/io.github.fastrizwaan.WineCharm/data/winecharm/Prefixes/$PFXNAME
PID=
for p in $(pgrep -f MapleStory.exe); do
  tr '\0' ' ' </proc/$p/cmdline 2>/dev/null | grep -q '^C:' && grep -qz "^WINEPREFIX=$PFX$" /proc/$p/environ 2>/dev/null && { PID=$p; break; }
done
[ -n "$PID" ] || { echo "$NAME: game not running"; exit 3; }
mkdir -p run logs
# max HP/MP are only remembered per level, per account: never share the default seed file between accounts
[ -f "neon-calib/seen-max.$NAME.json" ] || echo '{"level":0,"max_hp":0,"max_mp":0}' >"neon-calib/seen-max.$NAME.json"
LOG=run/view.$NAME.log
# never destroy a capture by restarting the view: keep the previous log (run/ is git-ignored)
if [ -s "$LOG" ]; then
  mkdir -p run/sessions
  STAMP=$NAME-$(date -r "$LOG" +%Y%m%d-%H%M%S)
  mv "$LOG" "run/sessions/$STAMP.log"
  [ -s "$LOG.wire" ] && mv "$LOG.wire" "run/sessions/$STAMP.log.wire"
fi
: >"$LOG"
# Own session, so stopping it (kill -- -PGID) cannot touch the caller and `kill 0` below only reaches these processes.
setsid bash -c '
  trap "kill 0" EXIT TERM INT
  NEON_DUMP_OPS=all nice -n 15 neon-stream/target/release/neon-stream --pid "$1" --chains neon-calib/chains.json --net --no-footholds \
    2> >(python3 neon-calib/tools/stamp.py "$2") >/dev/null &
  # memory source (position, HP/MP, level): only the newest line is kept
  nice -n 15 neon-stream/target/release/neon-stream --pid "$1" --chains neon-calib/chains.json --no-footholds --seen-file "neon-calib/seen-max.$4.json" 2>/dev/null \
    | python3 neon-calib/tools/latest.py "run/view.$4.mem.json" 5 &
  neon-view/target/release/neon-view --log "$2" --follow --speed 0 --port "$3" --mem "run/view.$4.mem.json" --marks run/marks.jsonl >"logs/view.$4.log" 2>&1 &
  wait
' _ "$PID" "$LOG" "$PORT" "$NAME" >/dev/null 2>&1 &
echo $! >"run/view.$NAME.pgid"
sleep 2
echo "$NAME: http://$(hostname -I | awk '{print $1}'):$PORT/  (game pid $PID; stop with: scripts/view.sh stop $NAME)"
