#!/usr/bin/env bash
# Start one extra account's client on its own headless X display and log it in.
#
#   instance-launch.sh NAME          (NAME is a key of neon-instances.json)
#
# Each instance has its own Wine prefix (scripts/instance-prefix.sh) and its own Xvfb display, so key presses,
# clicks and screenshots never reach another client or the desktop. Prints "pid <game pid> display :<n>" last.
# Then: DISPLAY=:<n> scripts/run-bot.sh --pid <game pid> --session NAME -- ...
set -eu
NAME=${1:?usage: instance-launch.sh NAME}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
read -r DISP CH < <(python3 -c "import json,sys; i=json.load(open('$ROOT/neon-instances.json'))[sys.argv[1]]; print(i['display'], i.get('channel', 40))" "$NAME")
# A registry "prefix" (and "charm") runs an existing prefix headless, e.g. the main one with its saved login.
read -r PFXNAME CHARM < <(python3 -c "import json,sys; i=json.load(open('$ROOT/neon-instances.json'))[sys.argv[1]]; print(i.get('prefix', 'NexonLauncher-'+sys.argv[1]), i.get('charm', 'nexon_launcher.charm'))" "$NAME")
export NEON_PREFIX=$HOME/.var/app/io.github.fastrizwaan.WineCharm/data/winecharm/Prefixes/$PFXNAME
export NEON_CHARM=$NEON_PREFIX/$CHARM
[ -d "$NEON_PREFIX" ] || "$ROOT/scripts/instance-prefix.sh" "$NAME"
# The launcher restores its last window position, which may be off the headless screen.
echo '{"width":1024,"height":720,"x":0,"y":0,"isMaximized":false,"isFullScreen":false,"displayBounds":{"x":0,"y":0,"width":1920,"height":1080}}' \
  >"$NEON_PREFIX/drive_c/users/$USER/AppData/Roaming/NexonLauncher/window-state.json"
export DISPLAY=:$DISP XAUTHORITY=
# Software rendering (no GPU on Xvfb) has no frame cap and takes every core it can get; cap its threads.
export LP_NUM_THREADS=${LP_NUM_THREADS:-2}
if ! xdpyinfo >/dev/null 2>&1; then
  mkdir -p "$ROOT/logs"
  setsid nohup Xvfb ":$DISP" -screen 0 1600x1000x24 -nolisten tcp -ac >"$ROOT/logs/xvfb-$NAME.log" 2>&1 &
  for _ in $(seq 20); do xdpyinfo >/dev/null 2>&1 && break; sleep 0.5; done
fi

# First run of a new prefix: the launcher shows its sign-in page. Sign in with the account from
# ~/.config/neon/accounts.json ("Login automatically" stays ticked, so this happens once).
launcher_win() { for w in $(xdotool search --name "Nexon Global Launcher" 2>/dev/null); do xdotool getwindowgeometry "$w" | grep -q 1024x720 && echo "$w"; done | head -1; }
mean() { import -window "$1" -crop "$2" +repage -colorspace Gray -format '%[fx:int(mean*100)]' info: 2>/dev/null || echo 0; }
if [ -z "$(launcher_win)" ]; then
  setsid nohup flatpak run io.github.fastrizwaan.WineCharm "$NEON_CHARM" >"$ROOT/logs/launcher-$NAME.log" 2>&1 &
  for _ in $(seq 60); do [ -n "$(launcher_win)" ] && break; sleep 2; done
  W=$(launcher_win)
  # Wait for the page to draw (white while loading), then look for the sign-in form: a white CONTINUE button
  # under a dark email field.
  for _ in $(seq 60); do [ "$(mean "$W" 1024x660+0+60)" -lt 60 ] && break; sleep 2; done
  if [ "$(mean "$W" 300x40+172+306)" -gt 90 ] && [ "$(mean "$W" 300x40+172+209)" -lt 20 ]; then
    echo "launcher: signing in ($NAME)"
    cred() { python3 -c "import json,sys; e=json.load(open(sys.argv[1]))[sys.argv[2]]['email']; a=[a for a in json.load(open(sys.argv[3]))['accounts'] if a['email']==e][0]; print(a[sys.argv[4]])" "$ROOT/neon-instances.json" "$NAME" "$HOME/.config/neon/accounts.json" "$1"; }
    xdotool mousemove --window "$W" 320 229 click 1; sleep 0.5
    xdotool type --delay 60 -- "$(cred email)"; sleep 0.5
    xdotool mousemove --window "$W" 320 326 click 1; sleep 5
    xdotool mousemove --window "$W" 300 289 click 1; sleep 0.5
    xdotool type --delay 70 -- "$(cred password)"; sleep 0.5
    xdotool mousemove --window "$W" 316 459 click 1; sleep 12
    [ "$(mean "$W" 300x40+172+306)" -gt 90 ] && { echo "launcher: still on the sign-in page (check $ROOT/logs, a CAPTCHA needs a person)"; exit 7; }
    # After signing in the launcher shows "All Games" (and a What's New popup): open the MapleStory page.
    for w in $(xdotool search --name "Nexon Global Launcher" 2>/dev/null); do
      xdotool getwindowgeometry "$w" | grep -q 802x526 && xdotool mousemove --window "$w" 734 499 click 1
    done
    sleep 1.5; xdotool mousemove --window "$W" 390 200 click 1; sleep 4
  fi
fi
# relaunch.sh takes an instance id for HWID spoofing; "none" has no HWID file, so nothing is changed.
"$ROOT/scripts/relaunch.sh" none 600
# Catch the login packet, which lists the whole inventory: a stream on the new client from before the login until
# it is in the game writes it to the character's --seen-file, where the bot's own stream picks it up.
GPID=$(for p in $(pgrep -f MapleStory.exe); do grep -qz "^WINEPREFIX=$NEON_PREFIX$" /proc/$p/environ 2>/dev/null && tr '\0' ' ' </proc/$p/cmdline | grep -q '^C:' && echo "$p"; done | head -1)
if [ -n "$GPID" ]; then
  nice -n 15 "$ROOT/neon-stream/target/release/neon-stream" --pid "$GPID" --chains "$ROOT/neon-calib/chains.json" --net --no-footholds \
    --seen-file "$ROOT/neon-calib/seen-max.$NAME.json" >/dev/null 2>>"$ROOT/logs/$NAME.login-stream.log" &
  LOGIN_STREAM=$!
fi
LOGIN_RC=0
"$ROOT/scripts/login.sh" "$CH" || LOGIN_RC=$?
sleep 12   # the inventory is saved within ~10 s of arriving
[ -n "${LOGIN_STREAM:-}" ] && kill "$LOGIN_STREAM" 2>/dev/null
[ "$LOGIN_RC" = 0 ] || exit "$LOGIN_RC"
PID=$(for p in $(pgrep -f MapleStory.exe); do grep -qz "^WINEPREFIX=$NEON_PREFIX$" /proc/$p/environ 2>/dev/null && tr '\0' ' ' </proc/$p/cmdline | grep -q '^C:' && echo "$p"; done | head -1)
# The launcher is not needed once the game runs (closing it leaves the client alone) and holds ~600 MB.
for p in $(pgrep -u "$(id -u)"); do
  grep -qz "^WINEPREFIX=$NEON_PREFIX$" /proc/$p/environ 2>/dev/null || continue
  case "$(tr '\0' ' ' </proc/$p/cmdline 2>/dev/null)" in *nexon_client*|*nexon_launcher*|*nexon_runtime*) kill "$p";; esac
done
echo "pid $PID display :$DISP"
