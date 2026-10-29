#!/usr/bin/env bash
# Relaunch MapleStory Classic World through the Nexon Launcher.
# Cross-platform: macOS (CrossOver / cliclick / screencapture) and Linux (WineCharm / xdotool).
# Uses the launcher's existing saved login; never touches credentials.
# Waits for PLAY to become enabled, clicks it once, then waits for MapleStory.exe.
# Exit 0 = game process is up.
#   usage: relaunch.sh [instance_id] [max_wait_seconds]   (default: default, 600)
set -u

INSTANCE="${1:-default}"
MAX="${2:-600}"

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

"$ROOT/scripts/apply-config.sh" "$INSTANCE" > /dev/null 2>&1 || true

case "$(uname -s)" in
    Darwin) PLATFORM="macos"; source "$ROOT/scripts/lib-macos-ui.sh" ;;
    Linux)  PLATFORM="linux" ;;
    *)      echo "Unsupported platform"; exit 1 ;;
esac

# ─── macOS implementation ────────────────────────────────────────────────

if [ "$PLATFORM" = "macos" ]; then

    game_up() { macos_game_pid; }
    launcher_running() { macos_launcher_running; }

    close_popups() {
        local popup; popup=$(macos_find_popup)
        if [ -n "$popup" ]; then
            echo "closing What's New popup"
            macos_click_window "$popup" 721 505
            sleep 1
        fi
    }

    ensure_classic_tab() {
        local win; win=$(macos_find_launcher)
        [ -z "$win" ] && return 1
        # Click "MapleStory Classic World" tab (right of "MapleStory" tab)
        macos_click_window "$win" 150 210
        sleep 1
    }

    click_play() {
        local win; win=$(macos_find_launcher)
        [ -z "$win" ] && return 1
        # PLAY button center: window-relative (140, 278) — calibrated on macOS Retina
        macos_click_window "$win" 140 278
    }

    if [ -n "$(game_up)" ]; then echo "game already running (pid $(game_up))"; exit 0; fi

    if ! launcher_running; then
        echo "starting Nexon launcher (macOS)..."
        macos_start_launcher
        for _ in $(seq 60); do launcher_running && break; sleep 1; done
    fi
    launcher_running || { echo "launcher never appeared"; exit 2; }

    # The launcher overwrites MachineGuid on startup — re-apply HWID after it settles
    echo "waiting for launcher to settle..."
    sleep 5
    if [ -x "$ROOT/scripts/hwid-spoof-advanced.sh" ]; then
        "$ROOT/scripts/hwid-spoof-advanced.sh" "$INSTANCE" apply > /dev/null 2>&1 || true
    fi

    START=$(date +%s)
    CLICKED=0
    while :; do
        [ -n "$(game_up)" ] && { echo "game is up (pid $(game_up))"; exit 0; }
        close_popups

        if [ $CLICKED -eq 0 ] || [ $(( $(date +%s) - CLICKED )) -ge 30 ]; then
            win=$(macos_find_launcher)
            if [ -n "$win" ]; then
                ensure_classic_tab
                echo "clicking PLAY..."
                click_play
                CLICKED=$(date +%s)
                # Wait 90s for the game to appear after clicking
                for i in $(seq 90); do
                    [ -n "$(game_up)" ] && { echo "game is up (pid $(game_up))"; exit 0; }
                    sleep 1
                done
                echo "MapleStory.exe not yet running; will retry..."
            fi
        fi

        [ $(( $(date +%s) - START )) -ge "$MAX" ] && { echo "gave up after ${MAX}s"; exit 5; }
        sleep 5
    done

# ─── Linux implementation ────────────────────────────────────────────────

else

    export DISPLAY=${DISPLAY:-:0}
    export XAUTHORITY=${XAUTHORITY:-$(ls /run/user/"$(id -u)"/.mutter-Xwaylandauth.* 2>/dev/null | head -1)}
    PFX=${NEON_PREFIX:-$HOME/.var/app/io.github.fastrizwaan.WineCharm/data/winecharm/Prefixes/NexonLauncherSetup-91dd6ddf36}
    CHARM=${NEON_CHARM:-$PFX/nexon_launcher_copy1.charm}

    game_up() {
        for p in $(pgrep -f "MapleStory.exe"); do
            c=$(tr '\0' ' ' </proc/$p/cmdline 2>/dev/null)
            case "$c" in 'C:\'*MapleStory.exe*) grep -qz "^WINEPREFIX=$PFX$" /proc/$p/environ 2>/dev/null && { echo "$p"; return 0; };; esac
        done
        return 1
    }
    launcher_win(){ xdotool search --name "Nexon Global Launcher" 2>/dev/null | head -1; }

    play_brightness() {
        local W=$(launcher_win); [ -z "$W" ] && return 1
        import -window "$W" "$TMP/w.png" 2>/dev/null || return 1
        convert "$TMP/w.png" -crop 200x48+50+267 +repage -colorspace Gray -format '%[fx:int(mean*1000)]' info:
    }
    tab_brightness() {
        convert "$TMP/w.png" -crop 98x44+"$1"+205 +repage -colorspace Gray -format '%[fx:int(mean*1000)]' info:
    }
    classic_selected() {
        local W=$(launcher_win)
        import -window "$W" "$TMP/w.png" 2>/dev/null || return 1
        local m c; m=$(tab_brightness 52); c=$(tab_brightness 152)
        [ "$c" -gt $(( m + 80 )) ]
    }
    ensure_classic() {
        classic_selected && return 0
        echo "Classic World tab not selected; selecting it"
        local W=$(launcher_win)
        xdotool mousemove --window "$W" 200 226 click 1; sleep 2
        classic_selected
    }
    close_popups() {
        for w in $(xdotool search --name "Nexon Global Launcher" 2>/dev/null); do
            case "$(xdotool getwindowgeometry "$w" 2>/dev/null | grep Geometry)" in
                *802x526*) echo "closing What's New popup ($w)"; xdotool mousemove --window "$w" 734 499 click 1; sleep 1;;
            esac
        done
    }

    if [ -n "$(game_up)" ]; then echo "game already running (pid $(game_up))"; exit 0; fi

    if [ -z "$(launcher_win)" ]; then
        echo "starting Nexon launcher..."
        nohup flatpak run io.github.fastrizwaan.WineCharm "$CHARM" >"$TMP/launch.log" 2>&1 &
        for _ in $(seq 60); do [ -n "$(launcher_win)" ] && break; sleep 1; done
    fi
    W=$(launcher_win); [ -z "$W" ] && { echo "launcher window never appeared"; exit 2; }

    START=$(date +%s)
    while :; do
        [ -n "$(game_up)" ] && { echo "game is up (pid $(game_up))"; exit 0; }
        B=$(play_brightness) || { sleep 3; B=0; }
        close_popups
        if [ "${B:-0}" -gt 220 ]; then
            ensure_classic || { echo "could not confirm Classic World is selected; NOT clicking PLAY"; exit 6; }
            cp "$TMP/w.png" "${RELAUNCH_SHOT:-/tmp/relaunch_before_click.png}"
            echo "PLAY enabled (brightness $B) and Classic World confirmed; clicking once"
            W=$(launcher_win)
            xdotool mousemove --window "$W" 150 291 click 1
            for _ in $(seq 90); do [ -n "$(game_up)" ] && { echo "game is up (pid $(game_up))"; exit 0; }; sleep 1; done
            echo "clicked PLAY but MapleStory.exe did not appear in 90s"; exit 4
        fi
        [ $(( $(date +%s) - START )) -ge "$MAX" ] && { echo "gave up after ${MAX}s: PLAY still disabled (brightness $B)"; exit 5; }
        sleep 10
    done
fi
