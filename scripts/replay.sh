#!/usr/bin/env bash
# Session browser: replay any saved capture in Neon View, pick it in the page.
#
#   scripts/replay.sh [PORT]           (default 8770; binds 0.0.0.0, no authentication)
#   scripts/replay.sh stop
#
# Lists run/sessions/ (archived captures; scripts/view.sh keeps the previous log when it restarts) plus the current live logs
# (linked in as live-NAME.log), and any *.nsr / *.jsonl dropped there. Loading a session shows it as of its end; use play / seek.
set -u
cd "$(dirname "$0")/.."
if [ "${1:-}" = stop ]; then
  [ -f run/replay.pid ] && kill "$(cat run/replay.pid)" 2>/dev/null; rm -f run/replay.pid; echo "replay browser stopped"; exit 0
fi
PORT=${1:-8770}
mkdir -p run/sessions
for f in run/view.*.log; do
  [ -e "$f" ] || continue
  n=$(basename "$f" .log); n=${n#view.}
  ln -sfn "../$(basename "$f")" "run/sessions/live-$n.log"
  [ -e "$f.wire" ] && ln -sfn "../$(basename "$f").wire" "run/sessions/live-$n.log.wire"
  [ -e "$f.wire.map" ] && ln -sfn "../$(basename "$f").wire.map" "run/sessions/live-$n.log.wire.map"
done
nohup neon-view/target/release/neon-view --sessions run/sessions --port "$PORT" >logs/replay.log 2>&1 &
echo $! >run/replay.pid
sleep 1
echo "replay browser: http://$(hostname -I | awk '{print $1}'):$PORT/  (stop with: scripts/replay.sh stop)"
