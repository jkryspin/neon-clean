#!/usr/bin/env bash
# Drive the MapleStory Classic World client from the login screen into the game:
#   Login -> channel -> "Go to the selected world" -> character -> Select.
# Cross-platform: macOS (cliclick / lib-macos-ui.sh) and Linux (xdotool).
# Uses the account the Nexon launcher already filled in; never types credentials.
# Safe to re-run: if the stats chain already reads HP, the character is in-game and
# nothing is clicked.
#   usage: login.sh [channel 1-40] [character slot 1]      (default: 40, 1)
set -u
CH=${1:-40}

ROOT=$(cd "$(dirname "$0")/.." && pwd)
STREAM=$ROOT/neon-stream/target/release/neon-stream
CHAINS=$ROOT/neon-calib/chains.json

case "$(uname -s)" in
    Darwin)
        PLATFORM="macos"
        source "$ROOT/scripts/lib-macos-ui.sh"
        ;;
    Linux)
        PLATFORM="linux"
        export DISPLAY=${DISPLAY:-:0}
        export XAUTHORITY=${XAUTHORITY:-$(ls /run/user/"$(id -u)"/.mutter-Xwaylandauth.* 2>/dev/null | head -1)}
        PFX=${NEON_PREFIX:-$HOME/.var/app/io.github.fastrizwaan.WineCharm/data/winecharm/Prefixes/NexonLauncherSetup-91dd6ddf36}
        ;;
    *)
        echo "Unsupported platform"; exit 1
        ;;
esac

# ─── Process detection ───────────────────────────────────────────────────

if [ "$PLATFORM" = "macos" ]; then
    game_pid() { macos_game_pid; }
else
    game_pid() {
        for p in $(pgrep -f "MapleStory.exe"); do
            c=$(tr '\0' ' ' </proc/$p/cmdline 2>/dev/null)
            case "$c" in 'C:\'*MapleStory.exe*) grep -qz "^WINEPREFIX=$PFX$" /proc/$p/environ 2>/dev/null && { echo "$p"; return 0; };; esac
        done; return 1
    }
fi

in_game() { [ -x "$STREAM" ] && nice -n 19 "$STREAM" --pid "$1" --chains "$CHAINS" --verify 2>/dev/null | grep -q 'hp .*= Some('; }

PID=$(game_pid) || { echo "MapleStory.exe not running (run scripts/relaunch.sh first)"; exit 2; }
in_game "$PID" && { echo "already in game"; exit 0; }

# ─── Window interaction ──────────────────────────────────────────────────

if [ "$PLATFORM" = "macos" ]; then
    TITLE_BAR=28  # macOS window title bar height in points
    click() {
        local x="$1" y="$2"
        local win; win=$(macos_find_game_window)
        if [ -n "$win" ]; then
            # Game coordinates are relative to game area; add title bar offset
            macos_click_window "$win" "$x" "$(( y + TITLE_BAR ))"
        else
            echo "WARN: game window not found for click at $x,$y"
        fi
    }
else
    for _ in $(seq 60); do
        W=$(for w in $(xdotool search --name "^MapleStory$" 2>/dev/null); do
              xdotool getwindowgeometry "$w" | grep -q 'Geometry: 1366x768' && echo "$w"; done | head -1)
        [ -n "$W" ] && break; sleep 1
    done
    [ -z "${W:-}" ] && { echo "game window not found"; exit 3; }
    click() { xdotool mousemove --window "$W" "$1" "$2" click 1; }
fi

# ─── Channel grid: 8 columns x 5 rows ───────────────────────────────────

COL=$(( (CH - 1) % 8 )); ROW=$(( (CH - 1) / 8 ))
CHX=$(( 457 + COL * 68 )); CHY=$(( 425 + ROW * 32 ))

sleep 15                         # let the login screen finish loading
echo "login";                 click 985 348;  sleep 6
echo "world";                 click 476 184;  sleep 3
echo "channel $CH";           click "$CHX" "$CHY"; sleep 2
echo "go to selected world";  click 888 374;  sleep 5
echo "select character";      click 492 423;  sleep 1.5
click 972 233
for _ in $(seq 30); do in_game "$PID" && { echo "in game"; exit 0; }; sleep 2; done
echo "did not reach the game; check the window"; exit 4
