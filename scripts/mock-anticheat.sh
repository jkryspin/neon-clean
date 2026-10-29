#!/usr/bin/env bash
# neon: disable the game's anti-cheat (GRAP + NGS) with mocks, before injecting.
#
#   scripts/mock-anticheat.sh install   swap in mock grap64.dll + nmcogame64.dll
#   scripts/mock-anticheat.sh restore   put the real DLLs back
#   scripts/mock-anticheat.sh status    show which files are mocked
#   scripts/mock-anticheat.sh inject    IAT-hook hook-session.dll into the launcher
#
# MapleStory.exe imports exactly two anti-cheat symbols (grap64.dll ordinal 9,
# nmcogame64.dll!NMCO_MemoryFree). The mocks satisfy those and nothing else, so
# the game runs with no protector watching the process. The launcher hook grabs
# the one-time session token out of the MapleStory.exe command line.
#
# Order: install with the game CLOSED (the DLLs are mapped while it runs);
# inject while the launcher is up and before clicking PLAY.
set -euo pipefail

APP="/Applications/MapleStory Launcher.app/Contents/SharedSupport/maplestoryna"
DRIVE="$HOME/Library/Application Support/MapleStoryNA/Bottles/maplestory/drive_c"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROBE="$ROOT/neon-probe"
GAME="$DRIVE/Nexon/Library/maplestorycw/appdata"

dput() { cp "$1" "$2" 2>/dev/null || sudo -n cp "$1" "$2" 2>/dev/null || { echo "copy failed: $2"; exit 1; }; }
build() { make -C "$PROBE" mock-grap64-win.dll mock-nmcogame64-win.dll hook-session-win.dll >/dev/null; }

case "${1:-status}" in
  install)
    build
    [ -d "$GAME" ] || { echo "game dir not found: $GAME"; exit 1; }
    for r in grap64.dll nmcogame64.dll; do
      m="mock-${r%.dll}-win.dll"
      [ -f "$GAME/$r.orig" ] || dput "$GAME/$r" "$GAME/$r.orig"
      dput "$PROBE/$m" "$GAME/$r"
      echo "mocked $r (real saved as $r.orig)"
    done
    ;;
  restore)
    for r in grap64.dll nmcogame64.dll; do
      if [ -f "$GAME/$r.orig" ]; then dput "$GAME/$r.orig" "$GAME/$r"; echo "restored $r"; fi
    done
    ;;
  status)
    for r in grap64.dll nmcogame64.dll; do
      if [ -f "$GAME/$r.orig" ]; then echo "$r: MOCKED"; else echo "$r: real"; fi
    done
    ;;
  inject)
    build
    x86_64-w64-mingw32-gcc -Wall -O2 -D_WIN32_WINNT=0x0600 -o "$PROBE/inject.exe" \
      "$PROBE/inject.c" -lpsapi -lkernel32
    mkdir -p "$DRIVE/Neon"
    dput "$PROBE/hook-session-win.dll" "$DRIVE/Neon/"
    dput "$PROBE/inject.exe" "$DRIVE/"
    for n in nexon_client.exe nexon_runtime.exe nexon_launcher.exe; do
      echo "-- $n"
      "$APP/bin/wine" --bottle default --wait-children "$DRIVE/inject.exe" \
        --all "$n" 'C:\Neon\hook-session-win.dll' || true
    done
    echo "watch: tail -f \"$DRIVE/tmp/neon-session-hook.\"*.log"
    ;;
  *) echo "usage: $0 install|restore|status|inject"; exit 2;;
esac
