#!/usr/bin/env bash
# Advanced HWID Spoofing for MapleStory Classic World
# Uses HWID Spoofer tool to modify system identifiers
# Usage: hwid-spoof-advanced.sh [instance_id] [action]

set -e

# Create directories early
HWID_DIR="$HOME/.neon/hwid"
mkdir -p "$HWID_DIR"

WINEPREFIX="${WINEPREFIX:-$HOME/.wine}"

# Parse arguments - handle 'list' as a special case (no instance_id needed)
if [ "$1" = "list" ] || [ "$1" = "help" ] || [ "$1" = "--help" ] || [ "$1" = "-h" ]; then
    ACTION="$1"
    INSTANCE_ID="default"
elif [ $# -ge 2 ]; then
    INSTANCE_ID="$1"
    ACTION="$2"
else
    INSTANCE_ID="${1:-default}"
    ACTION="apply"
fi

HWID_FILE="$HWID_DIR/${INSTANCE_ID}.json"

# Create directories
mkdir -p "$HWID_DIR"

# Generate unique hardware identifiers
generate_hwid() {
    local uuid=$(cat /proc/sys/kernel/random/uuid 2>/dev/null || python3 -c "import uuid; print(uuid.uuid4())")
    local mac=$(printf '%02x:%02x:%02x:%02x:%02x:%02x' $((0x02 + RANDOM % 254)) $((RANDOM % 256)) $((RANDOM % 256)) $((RANDOM % 256)) $((RANDOM % 256)) $((RANDOM % 256)))
    local disk_serial=$(printf 'S%08d%04d%04d' $((RANDOM * RANDOM % 100000000)) $((RANDOM % 10000)) $((RANDOM % 10000)))
    local cpu_id=$(printf '%08X-%04X-%04X-%04X-%012X' $((RANDOM * RANDOM % 4294967296)) $((RANDOM % 65536)) $((RANDOM % 65536)) $((RANDOM % 65536)) $((RANDOM * RANDOM % 281474976710656)))
    local motherboard_serial=$(printf 'MB-%04X-%04X-%04X' $((RANDOM % 65536)) $((RANDOM % 65536)) $((RANDOM % 65536)))

    cat << EOF
{
    "instance_id": "$INSTANCE_ID",
    "created": "$(date -Iseconds)",
    "hardware": {
        "uuid": "$uuid",
        "mac_address": "$mac",
        "disk_serial": "$disk_serial",
        "cpu_id": "$cpu_id",
        "motherboard_serial": "$motherboard_serial"
    },
    "wine": {
        "computer_name": "PC-$(echo $uuid | cut -c1-8 | tr '[:lower:]' '[:upper:]')",
        "username": "user_$INSTANCE_ID"
    }
}
EOF
}

# Initialize HWID
init_hwid() {
    echo "=== Initializing HWID for instance: $INSTANCE_ID ==="
    generate_hwid > "$HWID_FILE"
    echo "HWID saved to: $HWID_FILE"
    cat "$HWID_FILE"
}

# Apply HWID using Wine registry and system files
apply_hwid() {
    if [ ! -f "$HWID_FILE" ]; then
        echo "HWID file not found. Run: hwid-spoof-advanced.sh $INSTANCE_ID init"
        exit 1
    fi

    echo "=== Applying HWID for instance: $INSTANCE_ID ==="

    # Read values
    UUID=$(jq -r '.hardware.uuid' "$HWID_FILE")
    MAC_ADDR=$(jq -r '.hardware.mac_address' "$HWID_FILE")
    DISK_SERIAL=$(jq -r '.hardware.disk_serial' "$HWID_FILE")
    COMPUTER_NAME=$(jq -r '.wine.computer_name' "$HWID_FILE")

    # 1. Set Wine Computer Name in registry
    echo "Setting Wine registry values..."
    wine reg delete "HKLM\\SYSTEM\\CurrentControlSet\\Control\\ComputerName\\ComputerName" /v "ComputerName" /f 2>/dev/null || true
    wine reg add "HKLM\\SYSTEM\\CurrentControlSet\\Control\\ComputerName\\ComputerName" \
        /v "ComputerName" /t REG_SZ /d "$COMPUTER_NAME" /f 2>/dev/null || true

    wine reg delete "HKLM\\SYSTEM\\CurrentControlSet\\Control\\ComputerName\\ActiveComputerName" /v "ActiveComputerName" /f 2>/dev/null || true
    wine reg add "HKLM\\SYSTEM\\CurrentControlSet\\Control\\ComputerName\\ActiveComputerName" \
        /v "ActiveComputerName" /t REG_SZ /d "$COMPUTER_NAME" /f 2>/dev/null || true

    # 2. Update system.ini
    echo "Updating system.ini..."
    SYSTEM_INI="$WINEPREFIX/dosdevices/c:/windows/system.ini"
    if [ -f "$SYSTEM_INI" ]; then
        cp "$SYSTEM_INI" "${SYSTEM_INI}.hwid.bak"
        if grep -q "\[computername\]" "$SYSTEM_INI"; then
            sed -i 's/^ComputerName=.*/ComputerName='"$COMPUTER_NAME"'/' "$SYSTEM_INI"
        else
            echo "" >> "$SYSTEM_INI"
            echo "[computername]" >> "$SYSTEM_INI"
            echo "ComputerName=$COMPUTER_NAME" >> "$SYSTEM_INI"
        fi
    fi

    # 3. Update win.ini
    echo "Updating win.ini..."
    WIN_INI="$WINEPREFIX/dosdevices/c:/windows/win.ini"
    if [ -f "$WIN_INI" ]; then
        cp "$WIN_INI" "${WIN_INI}.hwid.bak"
        if grep -q "\[windows\]" "$WIN_INI"; then
            sed -i 's/^computer=.*/computer='"$COMPUTER_NAME"'/' "$WIN_INI"
        else
            echo "" >> "$WIN_INI"
            echo "[windows]" >> "$WIN_INI"
            echo "computer=$COMPUTER_NAME" >> "$WIN_INI"
        fi
    fi

    # 4. Set environment variables for this session
    export WINE_COMPUTER_NAME="$COMPUTER_NAME"

    echo ""
    echo "=== HWID Applied ==="
    echo "Computer Name: $COMPUTER_NAME"
    echo "UUID: $UUID"
    echo "MAC Address: $MAC_ADDR"
    echo ""
    echo "Note: For full HWID spoofing, consider using a tool like:"
    echo "  - HWID Spoofer (Windows executable in Wine)"
    echo "  - Or modify the Wine prefix's registry directly"
}

# Apply HWID and launch game
apply_and_launch() {
    apply_hwid
    echo ""
    echo "Launching MapleStory..."
    /home/john/neon/scripts/launch-game.sh "$@"
}

# List all HWIDs
list_hwids() {
    echo "=== HWID Instances ==="
    if [ ! -d "$HWID_DIR" ] || [ -z "$(ls -A $HWID_DIR 2>/dev/null)" ]; then
        echo "No HWID instances found."
        return
    fi

    for f in "$HWID_DIR"/*.json; do
        if [ -f "$f" ]; then
            id=$(basename "$f" .json)
            echo ""
            echo "--- Instance: $id ---"
            jq -c '{instance: .instance_id, uuid: .hardware.uuid, computer: .wine.computer_name, created: .created}' "$f"
        fi
    done
}

# Main
if [ "$ACTION" = "list" ]; then
    list_hwids
elif [ "$ACTION" = "help" ] || [ "$ACTION" = "--help" ] || [ "$ACTION" = "-h" ]; then
    echo "Usage: $0 [instance_id] [init|apply|launch|reset|list] [args...]"
    echo ""
    echo "Actions:"
    echo "  init    - Generate new HWID for instance"
    echo "  apply   - Apply HWID to Wine prefix"
    echo "  launch  - Apply HWID and launch game (pass channel as next arg)"
    echo "  reset   - Remove HWID for instance"
    echo "  list    - List all HWID instances"
    echo ""
    echo "Examples:"
    echo "  $0 bot1 init           # Create HWID for bot1"
    echo "  $0 bot1 apply          # Apply HWID to Wine"
    echo "  $0 bot1 launch 40      # Apply HWID and launch on channel 40"
    echo "  $0 list                # List all HWID instances"
elif [ "$ACTION" = "init" ]; then
    init_hwid
elif [ "$ACTION" = "apply" ]; then
    apply_hwid
elif [ "$ACTION" = "launch" ]; then
    shift
    apply_and_launch "$@"
elif [ "$ACTION" = "reset" ]; then
    if [ -f "$HWID_FILE" ]; then
        rm "$HWID_FILE"
        echo "HWID reset for instance: $INSTANCE_ID"
    else
        echo "No HWID file found for instance: $INSTANCE_ID"
    fi
else
    echo "Usage: $0 [instance_id] [init|apply|launch|reset|list] [args...]"
    echo ""
    echo "Actions:"
    echo "  init    - Generate new HWID for instance"
    echo "  apply   - Apply HWID to Wine prefix"
    echo "  launch  - Apply HWID and launch game (pass channel as next arg)"
    echo "  reset   - Remove HWID for instance"
    echo "  list    - List all HWID instances"
    echo ""
    echo "Examples:"
    echo "  $0 bot1 init           # Create HWID for bot1"
    echo "  $0 bot1 apply          # Apply HWID to Wine"
    echo "  $0 bot1 launch 40      # Apply HWID and launch on channel 40"
    exit 1
fi
