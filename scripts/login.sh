#!/usr/bin/env bash
# Drive the MapleStory Classic World client from the login screen into the game:
#   Login -> channel -> "Go to the selected world" -> character -> Select.
# Uses the account the Nexon launcher already filled in; never types credentials.
# Safe to re-run: if the stats chain already reads HP, the character is in-game and
# nothing is clicked.
#   usage: login.sh [channel 1-40] [character slot 1]      (default: 40, 1)
set -u
CH=${1:-40}
export DISPLAY=${DISPLAY:-:0}
export XAUTHORITY=${XAUTHORITY:-$(ls /run/user/"$(id -u)"/.mutter-Xwaylandauth.* 2>/dev/null | head -1)}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
STREAM=$ROOT/neon-stream/target/release/neon-stream
CHAINS=$ROOT/neon-calib/chains.json

PFX=${NEON_PREFIX:-$HOME/.var/app/io.github.fastrizwaan.WineCharm/data/winecharm/Prefixes/NexonLauncherSetup-91dd6ddf36}

# Real client = command line starting with a Windows path (not shells that mention it), running in our prefix.
game_pid() {
  for p in $(pgrep -f "MapleStory.exe"); do
    c=$(tr '\0' ' ' </proc/$p/cmdline 2>/dev/null)
    case "$c" in 'C:\'*MapleStory.exe*) grep -qz "^WINEPREFIX=$PFX$" /proc/$p/environ 2>/dev/null && { echo "$p"; return 0; };; esac
  done; return 1
}
in_game() { [ -x "$STREAM" ] && nice -n 19 "$STREAM" --pid "$1" --chains "$CHAINS" --verify 2>/dev/null | grep -q 'hp .*= Some('; }

PID=$(game_pid) || { echo "MapleStory.exe not running (run scripts/relaunch.sh first)"; exit 2; }
in_game "$PID" && { echo "already in game"; exit 0; }

# Inner game area is the 1366x768 window; the 1422x900 one is the title-bar frame.
for _ in $(seq 60); do
  W=$(for w in $(xdotool search --name "^MapleStory$" 2>/dev/null); do
        xdotool getwindowgeometry "$w" | grep -q 'Geometry: 1366x768' && echo "$w"; done | head -1)
  [ -n "$W" ] && break; sleep 1
done
[ -z "${W:-}" ] && { echo "game window not found"; exit 3; }
click() { xdotool mousemove --window "$W" "$1" "$2" click 1; }

# Coordinates are relative to the 1366x768 window. Channel grid: 8 columns x 5 rows.
COL=$(( (CH - 1) % 8 )); ROW=$(( (CH - 1) / 8 ))
CHX=$(( 457 + COL * 68 )); CHY=$(( 425 + ROW * 32 ))

sleep 15                         # let the login screen finish loading (slower headless)
echo "login";                 click 985 348;  sleep 6
echo "world";                 click 476 184;  sleep 3   # a new account starts with the list closed; a second click keeps it open
echo "channel $CH";           click "$CHX" "$CHY"; sleep 2
echo "go to selected world";  click 888 374;  sleep 5
echo "select character";      click 492 423;  sleep 1.5
click 972 233
for _ in $(seq 30); do in_game "$PID" && { echo "in game"; exit 0; }; sleep 2; done
echo "did not reach the game; check the window"; exit 4
