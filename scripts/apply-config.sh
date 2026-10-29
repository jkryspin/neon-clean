#!/usr/bin/env bash
# System configuration utility
# Cross-platform: macOS (CrossOver) and Linux (Wine)
# Usage: apply-config.sh [config_id]

set -e

CONFIG_DIR="$HOME/.neon/sys"

# Detect platform and set Wine paths
case "$(uname -s)" in
    Darwin)
        PLATFORM="macos"
        WINE_BIN="/Applications/MapleStory Launcher.app/Contents/SharedSupport/maplestoryna/MapleStory Launcher/wine"
        WINE_PFX="${NEON_PREFIX:-$HOME/Library/Application Support/MapleStoryNA/Bottles/maplestory}"
        WINE_SYS="$WINE_PFX/drive_c/windows"
        ;;
    Linux)
        PLATFORM="linux"
        WINE_BIN="wine"
        WINE_PFX="${NEON_PREFIX:-$HOME/.var/app/io.github.fastrizwaan.WineCharm/data/winecharm/Prefixes/NexonLauncherSetup-91dd6ddf36}"
        WINE_SYS="$WINE_PFX/dosdevices/c:/windows"
        ;;
    *)
        echo "ERROR: Unsupported platform: $(uname -s)"
        exit 1
        ;;
esac
export WINEPREFIX="$WINE_PFX"

WINE_REG="HKLM\\SYSTEM\\CurrentControlSet\\Control\\ComputerName"

# Portable sed -i
sed_i() {
    if [ "$PLATFORM" = "macos" ]; then
        sed -i '' "$@"
    else
        sed -i "$@"
    fi
}

# Get configuration for instance
get_config() {
    local id="$1"
    local file="$CONFIG_DIR/${id}.json"

    [ -f "$file" ] || return 1

    jq -r '.wine.computer_name' "$file" 2>/dev/null
}

# Apply system settings
apply_settings() {
    local name="$1"

    # Registry update
    "$WINE_BIN" reg delete "$WINE_REG\\ComputerName" /v "ComputerName" /f 2>/dev/null || true
    "$WINE_BIN" reg add "$WINE_REG\\ComputerName" /v "ComputerName" /t REG_SZ /d "$name" /f 2>/dev/null || true

    "$WINE_BIN" reg delete "$WINE_REG\\ActiveComputerName" /v "ActiveComputerName" /f 2>/dev/null || true
    "$WINE_BIN" reg add "$WINE_REG\\ActiveComputerName" /v "ActiveComputerName" /t REG_SZ /d "$name" /f 2>/dev/null || true

    # System files (resolve symlinks on macOS CrossOver bottles)
    local sys_ini="$WINE_SYS/system.ini"
    local win_ini="$WINE_SYS/win.ini"

    for ini in "$sys_ini" "$win_ini"; do
        [ -f "$ini" ] || continue
        if [ -L "$ini" ]; then
            local target; target=$(readlink "$ini")
            rm "$ini"; cp "$target" "$ini"
        fi
    done

    [ -f "$sys_ini" ] && {
        grep -q "\[computername\]" "$sys_ini" || { echo "" >> "$sys_ini"; echo "[computername]" >> "$sys_ini"; }
        sed_i "s/^ComputerName=.*/ComputerName=$name/" "$sys_ini" 2>/dev/null || echo "ComputerName=$name" >> "$sys_ini"
    }

    [ -f "$win_ini" ] && {
        grep -q "\[windows\]" "$win_ini" || { echo "" >> "$win_ini"; echo "[windows]" >> "$win_ini"; }
        sed_i "s/^computer=.*/computer=$name/" "$win_ini" 2>/dev/null || echo "computer=$name" >> "$win_ini"
    }
}

# Main
main() {
    local instance="${1:-default}"
    local config=$(get_config "$instance")

    if [ -n "$config" ]; then
        apply_settings "$config"
        echo "Configuration applied: $config"
    else
        echo "No configuration found for: $instance"
    fi
}

main "$@"
