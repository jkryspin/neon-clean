#!/usr/bin/env bash
# Verify HWID spoofing for running bots
# Cross-platform: macOS (CrossOver) and Linux (Wine)
# Usage: verify-hwid.sh [instance_id]

set -e

cd "$(dirname "$0")/.."
ROOT=$PWD

# Detect platform and set Wine paths
case "$(uname -s)" in
    Darwin)
        PLATFORM="macos"
        WINE_BIN="/Applications/MapleStory Launcher.app/Contents/SharedSupport/maplestoryna/MapleStory Launcher/wine"
        WINEPREFIX="${WINEPREFIX:-$HOME/Library/Application Support/MapleStoryNA/Bottles/maplestory}"
        ;;
    Linux)
        PLATFORM="linux"
        WINE_BIN="wine"
        WINEPREFIX="${WINEPREFIX:-$HOME/.wine}"
        ;;
    *)
        echo "ERROR: Unsupported platform: $(uname -s)"
        exit 1
        ;;
esac
export WINEPREFIX

# Query a registry value, stripping Windows \r line endings
reg_get() {
    local key="$1" name="$2"
    "$WINE_BIN" reg query "$key" /v "$name" 2>/dev/null | tr -d '\r' | grep "REG_SZ" | awk '{print $3}'
}

check_instance() {
    local instance="$1"
    local hwid_file="$HOME/.neon/hwid/${instance}.json"
    local ok=0 fail=0

    echo "=== Checking instance: $instance ($PLATFORM) ==="

    if [ ! -f "$hwid_file" ]; then
        echo "[FAIL] HWID file not found: $hwid_file"
        echo "       Run: $ROOT/scripts/hwid-spoof-advanced.sh $instance init"
        echo "       Then: $ROOT/scripts/hwid-spoof-advanced.sh $instance apply"
        return 1
    fi

    check_field() {
        local label="$1" expected="$2" actual="$3"
        if [ "$expected" = "null" ] || [ -z "$expected" ]; then
            printf "  %-20s [SKIP] not in HWID file\n" "$label"
        elif [ "$actual" = "$expected" ]; then
            printf "  %-20s [OK]   %s\n" "$label" "$actual"
            ok=$((ok + 1))
        else
            printf "  %-20s [FAIL] expected=%s actual=%s\n" "$label" "$expected" "$actual"
            fail=$((fail + 1))
        fi
    }

    local exp_computer=$(jq -r '.wine.computer_name' "$hwid_file")
    local exp_guid=$(jq -r '.hardware.machine_guid // empty' "$hwid_file")
    local exp_mac=$(jq -r '.hardware.mac_bare // empty' "$hwid_file")
    local exp_disk=$(jq -r '.hardware.disk_serial // empty' "$hwid_file")
    local exp_product=$(jq -r '.hardware.product_id // empty' "$hwid_file")
    local exp_hwprof=$(jq -r '.hardware.hw_profile_guid // empty' "$hwid_file")
    local exp_uuid=$(jq -r '.hardware.uuid // empty' "$hwid_file")
    local exp_vol=$(jq -r '.hardware.volume_serial // empty' "$hwid_file")

    local act_computer=$(reg_get "HKLM\\SYSTEM\\CurrentControlSet\\Control\\ComputerName\\ComputerName" "ComputerName")
    local act_guid=$(reg_get "HKLM\\SOFTWARE\\Microsoft\\Cryptography" "MachineGuid")
    local act_mac=$(reg_get "HKLM\\SYSTEM\\CurrentControlSet\\Control\\Class\\{4d36e972-e325-11ce-bfc1-08002be10318}\\0000" "NetworkAddress")
    local act_disk=$(reg_get "HKLM\\HARDWARE\\DEVICEMAP\\Scsi\\Scsi Port 0\\Scsi Bus 0\\Target Id 0\\Logical Unit Id 0" "SerialNumber")
    local act_product=$(reg_get "HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion" "ProductId")
    local act_hwprof=$(reg_get "HKLM\\SYSTEM\\CurrentControlSet\\Control\\IDConfigDB\\Hardware Profiles\\0001" "HwProfileGuid")
    local act_smbios=$(reg_get "HKLM\\SYSTEM\\CurrentControlSet\\Control\\SystemInformation" "ComputerHardwareId")
    local act_vol=$(cat "$WINEPREFIX/drive_c/.windows-serial" 2>/dev/null || echo "")

    echo ""
    check_field "ComputerName"  "$exp_computer" "$act_computer"
    check_field "MachineGuid"   "$exp_guid"     "$act_guid"
    check_field "MAC Address"   "$exp_mac"      "$act_mac"
    check_field "Disk Serial"   "$exp_disk"     "$act_disk"
    check_field "ProductId"     "$exp_product"  "$act_product"
    check_field "HW Profile"    "$exp_hwprof"   "$act_hwprof"
    check_field "SMBIOS UUID"   "{$exp_uuid}"   "$act_smbios"
    check_field "Volume Serial" "$exp_vol"      "$act_vol"

    echo ""
    if [ $fail -eq 0 ]; then
        echo "[OK] All $ok fields match — HWID spoofing is correctly applied."
        return 0
    else
        echo "[FAIL] $fail field(s) not applied ($ok OK)."
        echo "       Run: $ROOT/scripts/hwid-spoof-advanced.sh $instance apply"
        return 1
    fi
}

# Check all running sessions
check_all() {
    echo "=========================================="
    echo "HWID Verification ($PLATFORM)"
    echo "Wine prefix: $WINEPREFIX"
    echo "=========================================="
    echo ""

    local found=0
    for pidfile in "$ROOT/run"/*.pid; do
        if [ -f "$pidfile" ]; then
            local name=$(basename "$pidfile" .pid)
            check_instance "$name"
            echo ""
            found=1
        fi
    done

    if [ $found -eq 0 ]; then
        echo "No running bot sessions found in $ROOT/run/"
        echo "Checking all configured HWID instances instead..."
        echo ""
        for f in "$HOME/.neon/hwid"/*.json; do
            [ -f "$f" ] || continue
            check_instance "$(basename "$f" .json)"
            echo ""
        done
    fi
}

# Main
if [ $# -eq 0 ]; then
    check_all
else
    for instance in "$@"; do
        check_instance "$instance"
        echo ""
    done
fi
