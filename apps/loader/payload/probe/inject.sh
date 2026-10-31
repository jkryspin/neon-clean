#!/usr/bin/env bash
# neon-probe: inject a Windows DLL into a Wine process of the MapleStory bottle on this Mac.
#
#   scripts/inject.sh <image-name-or-win-pid> <dll>            load
#   scripts/inject.sh <image-name-or-win-pid> --unload <name>  unload
#
# Examples:
#   ./inject.sh cmd.exe hello-win.dll
#   ./inject.sh MapleStory.exe C:\Neon\probe_opcodes-win.dll
#
# How it works: inject.exe runs as a Wine process in the SAME bottle as the
# target and uses CreateRemoteThread + LoadLibraryW, which wineserver services
# directly - no debugger, no ptrace, nothing that trips the protector's
# anti-debug checks (see DESIGN.md for what has already failed that way).
#
# Gotcha: LoadLibrary refcounts. If the DLL is already loaded, LoadLibraryW
# returns the existing HMODULE and DllMain does NOT run again. Fully unload
# first (repeat --unload until it says the module is not found) if you expect
# a fresh DllMain.
set -eu
NAME=${1:?usage: inject.sh <image-name-or-win-pid> <dll> | --unload <module>}
ARG=${2:?missing dll path or --unload}

BOTTLE=default
APP="/Applications/MapleStory Launcher.app/Contents/SharedSupport/maplestoryna"
DRIVE="$HOME/Library/Application Support/MapleStoryNA/Bottles/maplestory/drive_c"
PROBE_DIR="$(cd "$(dirname "$0")" && pwd)"

# Build inject.exe if missing
if [ ! -f "$PROBE_DIR/inject.exe" ]; then
  (cd "$PROBE_DIR" && make inject.exe)
fi
cp "$PROBE_DIR/inject.exe" "$DRIVE/" 2>/dev/null || sudo -n cp "$PROBE_DIR/inject.exe" "$DRIVE/"

# A unix path is dropped into C:\Neon so the target can load it as C:\Neon\<name>.
WINARG=$ARG
if [ "$ARG" != "--unload" ] && [ "${ARG#/}" != "$ARG" ]; then
  [ -f "$ARG" ] || { echo "no such dll: $ARG"; exit 1; }
  mkdir -p "$DRIVE/Neon"
  cp "$ARG" "$DRIVE/Neon/"
  WINARG="C:\\Neon\\$(basename "$ARG")"
fi

exec "$APP/bin/wine" --bottle "$BOTTLE" --wait-children "$DRIVE/inject.exe" "$NAME" "$WINARG"
