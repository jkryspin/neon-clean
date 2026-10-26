#!/usr/bin/env bash
# Create a STR-first character on an instance's account and enter the game with it.
#
#   create-character.sh NAME [CHARACTER_NAME]      (NAME is a key of neon-instances.json)
#
# Run it on the character-select screen (instance-launch.sh stops there when the account has no character).
# Rolled stats are moved to STR (it caps at 12 at creation) and the rest to DEX, INT and LUK at the minimum 4.
# Then it picks the Classic key layout (arrow keys, Ins/Home quick slots: what the bot expects) and closes the
# tutorial dialog. Coordinates are in the 1366x768 game view.
set -eu
NAME=${1:?usage: create-character.sh NAME [CHARACTER_NAME]}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
read -r DISP CHAR < <(python3 -c "import json,sys; i=json.load(open('$ROOT/neon-instances.json'))[sys.argv[1]]; print(i['display'], i.get('character') or 'Kry' + sys.argv[1].capitalize())" "$NAME")
CHAR=${2:-$CHAR}
export DISPLAY=:$DISP XAUTHORITY=
W=$(for w in $(xdotool search --name "^MapleStory$" 2>/dev/null); do xdotool getwindowgeometry "$w" | grep -q 'Geometry: 1366x768' && echo "$w"; done | head -1)
[ -n "$W" ] || { echo "no game window on :$DISP"; exit 3; }
# A slow click: the client misses press/release pairs that come too fast on some buttons.
click() { xdotool mousemove --window "$W" "$1" "$2" sleep 0.3 mousedown 1 sleep 0.12 mouseup 1; sleep "${3:-0.4}"; }
shot() { import -window "$W" "$ROOT/logs/create-$NAME-$1.png" 2>/dev/null || true; }

echo "create a character ($CHAR)"
click 972 280 5                                   # "Create a character"
for _ in 1 2 3 4 5 6; do click 887 390 0.2; click 887 413 0.2; click 887 436 0.2; done   # DEX, INT, LUK down to 4
for _ in $(seq 12); do click 936 367 0.2; done     # STR up to its cap
for _ in $(seq 10); do click 936 390 0.2; done     # anything left to DEX
click 920 292; xdotool type --delay 80 "$CHAR"; sleep 0.5
click 987 256 2                                   # CHECK the name
shot name
click 651 411 2                                   # "This name is available" -> OK
click 880 549 5                                   # create
click 651 411 5                                   # confirm, if asked again
shot created
echo "enter the game"
click 492 423 1.5; click 972 233 10               # the character, Select
click 738 486 12                                  # key layout: Classic (preselected) -> CONFIRM
click 916 493 2                                   # tutorial dialog -> OK
shot ingame
python3 - "$ROOT/neon-instances.json" "$NAME" "$CHAR" <<'EOF'
import json, sys
p, name, char = sys.argv[1:]
d = json.load(open(p)); d[name]["character"] = char
json.dump(d, open(p, "w"), indent=2); open(p, "a").write("\n")
EOF
echo "done: $CHAR"
