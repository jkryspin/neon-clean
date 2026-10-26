#!/usr/bin/env bash
# Start, stop or show the bot of one instance (neon-instances.json).
#
#   instance-bot.sh NAME start [PLAN]   (default: the registry's "plan", else plans/maple-island-to-perion.json)
#   instance-bot.sh NAME stop
#   instance-bot.sh NAME status
#
# The game pid is the MapleStory.exe running in the instance's prefix, the display comes from the registry, and the
# largest HP seen is kept per character (neon-calib/seen-max.NAME.json). Stopping kills the recorded process group
# (run/NAME.pid) and releases the movement keys on the instance's display; nothing is matched by name.
set -u
NAME=${1:?usage: instance-bot.sh NAME start|stop|status [PLAN]}
CMD=${2:-status}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
DISP=$(python3 -c "import json,sys; print(json.load(open('neon-instances.json'))[sys.argv[1]]['display'])" "$NAME") || exit 2
# HP potions: "auto" finds them in the server's inventory and puts them on Ins; a registry hp_cell overrides.
HP_CELL=$(python3 -c "import json,sys; print(json.load(open('neon-instances.json'))[sys.argv[1]].get('hp_cell', 'auto'))" "$NAME")
reg() { python3 -c "import json,sys; print(json.load(open('neon-instances.json'))[sys.argv[1]].get(sys.argv[2], sys.argv[3]))" "$NAME" "$1" "$2"; }
PFX=$HOME/.var/app/io.github.fastrizwaan.WineCharm/data/winecharm/Prefixes/$(reg prefix "NexonLauncher-$NAME")
MP_KEY=$(reg mp_key Home)
game_pid() {
  for p in $(pgrep -f MapleStory.exe); do
    tr '\0' ' ' </proc/$p/cmdline 2>/dev/null | grep -q '^C:' && grep -qz "^WINEPREFIX=$PFX$" /proc/$p/environ 2>/dev/null && { echo "$p"; return 0; }
  done
  return 1
}
running() { [ -f "run/$NAME.pid" ] && kill -0 "$(cat "run/$NAME.pid")" 2>/dev/null; }

case "$CMD" in
  start)
    running && { echo "$NAME: already running (group $(cat "run/$NAME.pid"))"; exit 0; }
    PID=$(game_pid) || { echo "$NAME: game not running (scripts/instance-launch.sh $NAME)"; exit 3; }
    PLAN=${3:-$(reg plan plans/maple-island-to-perion.json)}
    [ -f "neon-calib/seen-max.$NAME.json" ] || echo '{"level":0,"max_hp":0,"max_mp":0}' >"neon-calib/seen-max.$NAME.json"
    mkdir -p logs
    DISPLAY=:$DISP XAUTHORITY= NEON_STREAM_ARGS="--seen-file neon-calib/seen-max.$NAME.json" \
      setsid nohup scripts/run-bot.sh --session "$NAME" --pid "$PID" -- --plan "$PLAN" --hp-key "${NEON_HP_KEY:-$HP_CELL}" --mp-key "$MP_KEY" --auto-revive --build builds/warrior.json ${NEON_BOT_ARGS:-} \
      ${NEON_TRACE_DIR:+--trace-file "$NEON_TRACE_DIR/$NAME.trace"} >>"logs/$NAME.run.log" 2>&1 &
    sleep 2
    echo "$NAME: started on :$DISP for game pid $PID (logs/$NAME.run.log)"
    ;;
  stop)
    if running; then
      G=$(cat "run/$NAME.pid")
      kill -TERM -- "-$G" 2>/dev/null
      # run-bot.sh removes run/NAME.pid in its exit trap, which bash runs only after the pipeline has ended: wait for
      # the whole group, or a quick restart has its new pid file deleted by the old trap.
      for _ in $(seq 50); do pgrep -g "$G" >/dev/null || break; sleep 0.2; done
    fi
    DISPLAY=:$DISP XAUTHORITY= xdotool keyup Left Right Up Down Alt_L Control_L z 2>/dev/null
    echo "$NAME: stopped"
    ;;
  status)
    running && echo "$NAME: running (group $(cat "run/$NAME.pid"))" || echo "$NAME: not running"
    python3 - "$NAME" <<'EOF'
import json, sys
try:
    d = json.load(open(f"neon-status.{sys.argv[1]}.json"))
    p = d["state"]["player"]
    print(f"  map {p['map_id']} at ({p['x']}, {p['y']}), level {p['level']}, HP {p['hp']}/{p['max_hp']}, mode {d.get('mode')}")
except Exception as e:
    print(f"  no status ({e})")
EOF
    ;;
  *) echo "unknown command $CMD"; exit 2;;
esac
