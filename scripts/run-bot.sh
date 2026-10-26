#!/usr/bin/env bash
# Start the bot pipeline (neon-stream | neon-loader) for one session, optionally supervised.
#
#   run-bot.sh [--supervise] [--session NAME] [--instance INSTANCE_ID] [--pid GAME_PID] [-- loader args...]
#
# --pid            drive this game process (several clients: one session per client); default: the first one found
# --instance       HWID instance ID for this bot (used for HWID spoofing when relaunching)
# --supervise      restart the pipeline if it crashes (not when it stops on purpose: stop button, plan finished,
#                  target level, panic key), relaunch the game first if it is not running, at most 5 restarts/hour.
# The process-group id is written to run/<session>.pid so the web UI (or you) can stop it with
#   kill -TERM -- -$(cat run/main.pid)
# Everything runs niced; the game is only read, never written.
set -u
cd "$(dirname "$0")/.."
ROOT=$PWD
SESSION=main
INSTANCE=default
SUPERVISE=0
FORCE_PID=
while [ $# -gt 0 ]; do
  case "$1" in
    --supervise) SUPERVISE=1; shift;;
    --session) SESSION=$2; shift 2;;
    --instance) INSTANCE=$2; shift 2;;
    --pid) FORCE_PID=$2; shift 2;;
    --) shift; break;;
    *) break;;
  esac
done
case "$SESSION" in *[!a-zA-Z0-9_-]*|"") echo "bad session name"; exit 2;; esac
mkdir -p run history logs
echo $$ >"run/$SESSION.pid"
trap 'rm -f "run/$SESSION.pid"; kill 0 2>/dev/null' EXIT INT TERM

game_pid() {
  if [ -n "$FORCE_PID" ]; then kill -0 "$FORCE_PID" 2>/dev/null && echo "$FORCE_PID" && return 0; return 1; fi
  for p in $(pgrep -f "MapleStory.exe"); do
    c=$(tr '\0' ' ' </proc/$p/cmdline 2>/dev/null)
    case "$c" in 'C:\'*MapleStory.exe*) echo "$p"; return 0;; esac
  done
  return 1
}

# Relaunch game with HWID spoofing
relaunch_game() {
  local max_wait="${1:-900}"
  echo "$(date +%T) applying config for instance $INSTANCE..."
  "$ROOT/scripts/apply-config.sh" "$INSTANCE" > /dev/null 2>&1 || {
    echo "$(date +%T) config apply completed"
  }
  echo "$(date +%T) launching game..."
  "$ROOT/scripts/relaunch.sh" "$INSTANCE" "$max_wait" || return 1
}

export DISPLAY=${DISPLAY:-:0}
export XAUTHORITY=${XAUTHORITY:-$(ls /run/user/"$(id -u)"/.mutter-Xwaylandauth.* 2>/dev/null | head -1)}
STREAM=$ROOT/neon-stream/target/release/neon-stream
LOADER=$ROOT/neon-loader/target/release/neon-loader
[ -x "$STREAM" ] && [ -x "$LOADER" ] || { echo "build first: cargo build --release in neon-stream and neon-loader"; exit 2; }

restarts=()
while :; do
  PID=$(game_pid) || PID=""
  if [ -z "$PID" ]; then
    if [ "$SUPERVISE" = 1 ]; then
      relaunch_game 900 || { echo "$(date +%T) relaunch failed"; sleep 30; continue; }
      sleep 20; PID=$(game_pid) || continue
    else
      echo "MapleStory is not running"; exit 3
    fi
  fi
  echo "$(date +%T) starting pipeline for game pid $PID (session $SESSION, instance $INSTANCE)"
  nice -n 15 "$STREAM" --pid "$PID" --chains "$ROOT/neon-calib/chains.json" --net --no-footholds ${NEON_STREAM_ARGS:-} 2>>"logs/$SESSION.stream.log" \
    | "$LOADER" --stdin --session "$SESSION" --maps-dir "$ROOT/neon-calib/maps" --history-dir "$ROOT/history" "$@" 2>&1 | tee -a "logs/$SESSION.log"
  code=${PIPESTATUS[1]}
  [ "$code" = 0 ] && { echo "$(date +%T) loader finished normally"; exit 0; }
  [ "$SUPERVISE" = 1 ] || exit "$code"
  now=$(date +%s); restarts=("${restarts[@]}" "$now")
  recent=(); for t in "${restarts[@]}"; do [ $((now - t)) -lt 3600 ] && recent=("${recent[@]}" "$t"); done; restarts=("${recent[@]}")
  if [ "${#restarts[@]}" -gt 5 ]; then echo "$(date +%T) more than 5 restarts in an hour: giving up"; exit 4; fi
  echo "$(date +%T) loader exited with $code: restarting in 5 s"; sleep 5
done
