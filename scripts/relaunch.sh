#!/usr/bin/env bash
# Relaunch MapleStory Classic World through the Nexon Launcher (WineCharm flatpak).
# Uses the launcher's existing saved login; never touches credentials.
# Waits for PLAY to become enabled (it is disabled while the launcher believes a
# game session is still running, e.g. after a crash), clicks it once, then waits
# for MapleStory.exe. Exit 0 = game process is up.
#   usage: relaunch.sh [instance_id] [max_wait_seconds]   (default: default, 600)
# NEON_PREFIX / NEON_CHARM select another account's prefix (scripts/instance-prefix.sh); only a game process
# running in that prefix counts as "the game".
set -u

# Parse arguments
INSTANCE="${1:-default}"
MAX="${2:-600}"

export DISPLAY=${DISPLAY:-:0}
export XAUTHORITY=${XAUTHORITY:-$(ls /run/user/"$(id -u)"/.mutter-Xwaylandauth.* 2>/dev/null | head -1)}
PFX=${NEON_PREFIX:-$HOME/.var/app/io.github.fastrizwaan.WineCharm/data/winecharm/Prefixes/NexonLauncherSetup-91dd6ddf36}
CHARM=${NEON_CHARM:-$PFX/nexon_launcher_copy1.charm}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

# Apply system configuration before launch
apply_config() {
    local instance="$1"
    "$ROOT/scripts/apply-config.sh" "$instance" > /dev/null 2>&1 || true
}

# Apply config before launching
apply_config "$INSTANCE"

# The real client's command line starts with a Windows path (C:\...\MapleStory.exe).
# Shells that merely mention the name must not count.
game_up() {
  for p in $(pgrep -f "MapleStory.exe"); do
    c=$(tr '\0' ' ' </proc/$p/cmdline 2>/dev/null)
    case "$c" in 'C:\'*MapleStory.exe*) grep -qz "^WINEPREFIX=$PFX$" /proc/$p/environ 2>/dev/null && { echo "$p"; return 0; };; esac
  done
  return 1
}
launcher_win(){ xdotool search --name "Nexon Global Launcher" 2>/dev/null | head -1; }

if [ -n "$(game_up)" ]; then echo "game already running (pid $(game_up))"; exit 0; fi

if [ -z "$(launcher_win)" ]; then
  echo "starting Nexon launcher..."
  nohup flatpak run io.github.fastrizwaan.WineCharm "$CHARM" >"$TMP/launch.log" 2>&1 &
  for _ in $(seq 60); do [ -n "$(launcher_win)" ] && break; sleep 1; done
fi
W=$(launcher_win); [ -z "$W" ] && { echo "launcher window never appeared"; exit 2; }

# PLAY button region in the 1024x720 launcher window: x 50..250, y 267..315.
# Disabled = dark grey (mean brightness low); enabled = bright.
play_brightness() {
  import -window "$W" "$TMP/w.png" 2>/dev/null || return 1
  convert "$TMP/w.png" -crop 200x48+50+267 +repage -colorspace Gray -format '%[fx:int(mean*1000)]' info:
}

# Which game is selected? The pill toggle under the MapleStory logo has two tabs:
# "MapleStory" (x 52..150) and "Classic World" (x 152..250). The selected tab is the
# lighter pill. PLAY launches whichever is selected, so verify before clicking.
tab_brightness() {  # $1 = x offset of the tab
  convert "$TMP/w.png" -crop 98x44+"$1"+205 +repage -colorspace Gray -format '%[fx:int(mean*1000)]' info:
}
classic_selected() {
  import -window "$W" "$TMP/w.png" 2>/dev/null || return 1
  local m c; m=$(tab_brightness 52); c=$(tab_brightness 152)
  [ "$c" -gt $(( m + 80 )) ]
}
ensure_classic() {
  classic_selected && return 0
  echo "Classic World tab not selected; selecting it"
  xdotool mousemove --window "$W" 200 226 click 1; sleep 2
  classic_selected
}

# The launcher opens a "What's New" news popup (802x526) on start; it sits on top of
# the main window and swallows clicks. Close it via its Close button.
close_popups() {
  for w in $(xdotool search --name "Nexon Global Launcher" 2>/dev/null); do
    case "$(xdotool getwindowgeometry "$w" 2>/dev/null | grep Geometry)" in
      *802x526*) echo "closing What's New popup ($w)"; xdotool mousemove --window "$w" 734 499 click 1; sleep 1;;
    esac
  done
}

START=$(date +%s)
while :; do
  [ -n "$(game_up)" ] && { echo "game is up (pid $(game_up))"; exit 0; }
  B=$(play_brightness) || { sleep 3; B=0; }  # window may still be drawing; retry until MAX
  close_popups
  if [ "${B:-0}" -gt 220 ]; then
    ensure_classic || { echo "could not confirm MapleStory Classic World is selected; NOT clicking PLAY"; exit 6; }
    cp "$TMP/w.png" "${RELAUNCH_SHOT:-/tmp/relaunch_before_click.png}"
    echo "PLAY enabled (brightness $B) and Classic World confirmed; clicking once"
    xdotool mousemove --window "$W" 150 291 click 1
    for _ in $(seq 90); do [ -n "$(game_up)" ] && { echo "game is up (pid $(game_up))"; exit 0; }; sleep 1; done
    echo "clicked PLAY but MapleStory.exe did not appear in 90s"; exit 4
  fi
  [ $(( $(date +%s) - START )) -ge "$MAX" ] && { echo "gave up after ${MAX}s: PLAY still disabled (brightness $B)"; exit 5; }
  sleep 10
done
