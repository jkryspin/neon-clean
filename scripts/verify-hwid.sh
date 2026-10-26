#!/usr/bin/env bash
# Verify HWID spoofing for running bots
# Usage: verify-hwid.sh [instance_id]

set -e

cd "$(dirname "$0")/.."
ROOT=$PWD

check_instance() {
    local instance="$1"
    local hwid_file="$HOME/.neon/hwid/${instance}.json"

    echo "=== Checking instance: $instance ==="

    # Check if HWID file exists
    if [ -f "$hwid_file" ]; then
        echo "[OK] HWID file exists: $hwid_file"
        local expected_computer=$(jq -r '.wine.computer_name' "$hwid_file")
        echo "[OK] Expected computer name: $expected_computer"
    else
        echo "[WARN] HWID file not found: $hwid_file"
        echo "       This instance was started before HWID spoofing was configured"
        expected_computer="unknown"
    fi

    # Check Wine registry
    local reg_computer=$(wine reg query "HKLM\\SYSTEM\\CurrentControlSet\\Control\\ComputerName\\ComputerName" 2>/dev/null | grep "ComputerName    REG_SZ" | awk '{print $3}')
    echo ""
    echo "Wine Registry ComputerName: $reg_computer"

    # Check system.ini
    local sys_computer=$(grep "^ComputerName=" ~/.wine/dosdevices/c:/windows/system.ini 2>/dev/null | cut -d= -f2 || echo "not found")
    echo "system.ini ComputerName: $sys_computer"

    # Check win.ini
    local win_computer=$(grep "^computer=" ~/.wine/dosdevices/c:/windows/win.ini 2>/dev/null | cut -d= -f2 || echo "not found")
    echo "win.ini computer: $win_computer"

    # Verify match
    echo ""
    if [ "$reg_computer" = "$expected_computer" ] && [ "$expected_computer" != "unknown" ]; then
        echo "[OK] HWID spoofing is correctly applied!"
        return 0
    elif [ "$sys_computer" = "$expected_computer" ] && [ "$expected_computer" != "unknown" ]; then
        echo "[WARN] Partial spoofing: system.ini matches but registry doesn't"
        echo "       The registry may need a Wine restart to update"
        return 0
    elif [ "$sys_computer" != "not found" ] && [ "$expected_computer" != "unknown" ]; then
        echo "[INFO] HWID applied (system.ini shows: $sys_computer)"
        echo "       Note: Multiple bots sharing the same Wine prefix will have the last-applied HWID"
        echo "       For true per-bot HWID, each bot needs its own Wine prefix"
        return 0
    else
        echo "[FAIL] HWID spoofing not applied or mismatch"
        echo "       Run: $ROOT/scripts/hwid-spoof-advanced.sh $instance init"
        echo "       Then: $ROOT/scripts/hwid-spoof-advanced.sh $instance apply"
        return 1
    fi
}

# Check all running sessions
check_all() {
    echo "=========================================="
    echo "HWID Verification for All Running Bots"
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
    fi

    echo "=========================================="
    echo "HWID Instances Available:"
    echo "=========================================="
    "$ROOT/scripts/hwid-spoof-advanced.sh" list 2>/dev/null || echo "No HWID instances configured"

    echo ""
    echo "Note: If multiple bots share the same Wine prefix, only the last-applied HWID"
    echo "will be active. For true per-bot HWID isolation, each bot needs its own Wine prefix."
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
