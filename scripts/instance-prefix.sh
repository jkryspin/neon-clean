#!/usr/bin/env bash
# Make a Wine prefix for one more game account (a Neon "instance"), next to the main launcher prefix.
#
#   instance-prefix.sh NAME
#
# The copy keeps the launcher's settings (which games are installed and where) but not its login: cookies,
# web storage and the session file are left out, so the new launcher starts signed out and is signed into its own
# account without touching the main one. The game's data files (MapleStory .wz, ~700 MB) are symlinked to the main
# prefix rather than copied; patching happens in the main prefix. Logs (several GB) are not copied.
# Nothing that identifies the machine is changed. The launcher runs with --disable-gpu: its embedded browser draws
# a blank white page on a display without GPU access (Xvfb).
set -eu
NAME=${1:?usage: instance-prefix.sh NAME}
case "$NAME" in *[!a-zA-Z0-9_-]*) echo "bad name"; exit 2;; esac
PFXS=$HOME/.var/app/io.github.fastrizwaan.WineCharm/data/winecharm/Prefixes
BASE=$PFXS/NexonLauncherSetup-91dd6ddf36
NEW=$PFXS/NexonLauncher-$NAME
DATA=drive_c/Nexon/Library/maplestorycw/appdata/Data
LAUNCHER=drive_c/users/$USER/AppData/Roaming/NexonLauncher
[ -e "$NEW" ] && { echo "$NEW already exists"; exit 0; }

nice rsync -a \
  --exclude '/*.log' --exclude '/*.charm' --exclude "/$DATA" \
  --exclude "/$LAUNCHER/Network" --exclude "/$LAUNCHER/Local Storage" --exclude "/$LAUNCHER/Session Storage" \
  --exclude "/$LAUNCHER/WebStorage" --exclude "/$LAUNCHER/databases" --exclude "/$LAUNCHER/blob_storage" \
  --exclude "/$LAUNCHER/nxa_session.db" --exclude "/$LAUNCHER/lockfile" \
  --exclude "/$LAUNCHER/Cache" --exclude "/$LAUNCHER/Code Cache" --exclude "/$LAUNCHER/GPUCache" --exclude "/$LAUNCHER/DawnCache" \
  "$BASE/" "$NEW/"
cp -as "$BASE/$DATA" "$NEW/$DATA"

sed -e "s#NexonLauncherSetup-91dd6ddf36#NexonLauncher-$NAME#g" -e "s#'progname': '.*'#'progname': 'nexon_launcher $NAME'#" -e 's#nexon_launcher_copy1.charm#nexon_launcher.charm#' \
  -e "s#'args': ''#'args': '--disable-gpu'#" \
  "$BASE/nexon_launcher_copy1.charm" >"$NEW/nexon_launcher.charm"
# The launcher restores its last window position, which is off-screen on a smaller (headless) display.
echo '{"width":1024,"height":720,"x":0,"y":0,"isMaximized":false,"isFullScreen":false,"displayBounds":{"x":0,"y":0,"width":1920,"height":1080}}' \
  >"$NEW/$LAUNCHER/window-state.json"
echo "made $NEW ($(du -sh "$NEW" | cut -f1))"
