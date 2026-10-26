#!/usr/bin/env bash
# System configuration utility
# Usage: apply-config.sh [config_id]

set -e

CONFIG_DIR="$HOME/.neon/sys"
WINE_PFX="${NEON_PREFIX:-$HOME/.var/app/io.github.fastrizwaan.WineCharm/data/winecharm/Prefixes/NexonLauncherSetup-91dd6ddf36}"
WINE_SYS="$WINE_PFX/dosdevices/c:/windows"
WINE_REG="HKLM\\SYSTEM\\CurrentControlSet\\Control\\ComputerName"

# Get configuration for instance
get_config() {
    local id="$1"
    local file="$CONFIG_DIR/${id}.json"

    [ -f "$file" ] || return 1

    # Read config data
    jq -r '.wine.computer_name' "$file" 2>/dev/null
}

# Apply system settings
apply_settings() {
    local name="$1"

    # Registry update
    WINEPREFIX="$WINE_PFX" wine reg delete "$WINE_REG\\ComputerName" /v "ComputerName" /f 2>/dev/null || true
    WINEPREFIX="$WINE_PFX" wine reg add "$WINE_REG\\ComputerName" /v "ComputerName" /t REG_SZ /d "$name" /f 2>/dev/null || true

    WINEPREFIX="$WINE_PFX" wine reg delete "$WINE_REG\\ActiveComputerName" /v "ActiveComputerName" /f 2>/dev/null || true
    WINEPREFIX="$WINE_PFX" wine reg add "$WINE_REG\\ActiveComputerName" /v "ActiveComputerName" /t REG_SZ /d "$name" /f 2>/dev/null || true

    # System files
    local sys_ini="$WINE_SYS/system.ini"
    local win_ini="$WINE_SYS/win.ini"

    [ -f "$sys_ini" ] && {
        grep -q "\[computername\]" "$sys_ini" || { echo "" >> "$sys_ini"; echo "[computername]" >> "$sys_ini"; }
        sed -i "s/^ComputerName=.*/ComputerName=$name/" "$sys_ini" 2>/dev/null || echo "ComputerName=$name" >> "$sys_ini"
    }

    [ -f "$win_ini" ] && {
        grep -q "\[windows\]" "$win_ini" || { echo "" >> "$win_ini"; echo "[windows]" >> "$win_ini"; }
        sed -i "s/^computer=.*/computer=$name/" "$win_ini" 2>/dev/null || echo "computer=$name" >> "$win_ini"
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
