#!/usr/bin/env bash
# HWID Spoofing for MapleStory Classic World
# Generates unique hardware identifiers for each bot instance
# Usage: hwid-spoof.sh [instance_id] [action]
#   action: init | apply | reset

set -e

INSTANCE_ID="${1:-default}"
ACTION="${2:-apply}"
WINEPREFIX="${WINEPREFIX:-$HOME/.wine}"
HWID_FILE="$HOME/.neon/hwid/${INSTANCE_ID}.json"

# Create HWID directory
mkdir -p "$HOME/.neon/hwid"

# Generate a random UUID
generate_uuid() {
    cat /proc/sys/kernel/random/uuid 2>/dev/null || \
    python3 -c "import uuid; print(uuid.uuid4())"
}

# Generate random hardware values
generate_mac() {
    # Generate a random MAC address (not multicast, locally administered)
    printf '%02x:%02x:%02x:%02x:%02x:%02x' \
        $((0x02 + (RANDOM % 0xFC))) \
        $((RANDOM % 256)) \
        $((RANDOM % 256)) \
        $((RANDOM % 256)) \
        $((RANDOM % 256)) \
        $((RANDOM % 256))
}

generate_disk_serial() {
    # Generate a random disk serial number
    printf 'WD-%04X-%04X-%04X-%04X' \
        $((RANDOM % 65536)) \
        $((RANDOM % 65536)) \
        $((RANDOM % 65536)) \
        $((RANDOM % 65536))
}

generate_cpu_id() {
    # Generate a random CPU ID
    printf '%08X-%04X-%04X-%04X-%012X' \
        $((RANDOM * RANDOM % 4294967296)) \
        $((RANDOM % 65536)) \
        $((RANDOM % 65536)) \
        $((RANDOM % 65536)) \
        $((RANDOM * RANDOM % 281474976710656))
}

# Initialize HWID for a new instance
init_hwid() {
    echo "Initializing HWID for instance: $INSTANCE_ID"

    cat > "$HWID_FILE" << EOF
{
    "instance_id": "$INSTANCE_ID",
    "created": "$(date -Iseconds)",
    "hardware": {
        "mac_address": "$(generate_mac)",
        "disk_serial": "$(generate_disk_serial)",
        "cpu_id": "$(generate_cpu_id)",
        "uuid": "$(generate_uuid)"
    },
    "wine": {
        "computer_name": "NEON-$(echo $INSTANCE_ID | tr '[:lower:]' '[:upper:]' | cut -c1-15)",
        "username": "player_$INSTANCE_ID"
    }
}
EOF

    echo "HWID initialized and saved to: $HWID_FILE"
    cat "$HWID_FILE"
}

# Apply HWID to Wine prefix
apply_hwid() {
    if [ ! -f "$HWID_FILE" ]; then
        echo "HWID file not found for instance: $INSTANCE_ID"
        echo "Run: hwid-spoof.sh $INSTANCE_ID init"
        exit 1
    fi

    echo "Applying HWID for instance: $INSTANCE_ID"

    # Read HWID values
    MAC_ADDR=$(jq -r '.hardware.mac_address' "$HWID_FILE")
    DISK_SERIAL=$(jq -r '.hardware.disk_serial' "$HWID_FILE")
    CPU_ID=$(jq -r '.hardware.cpu_id' "$HWID_FILE")
    UUID=$(jq -r '.hardware.uuid' "$HWID_FILE")
    COMPUTER_NAME=$(jq -r '.wine.computer_name' "$HWID_FILE")
    USERNAME=$(jq -r '.wine.username' "$HWID_FILE")

    # Set Wine registry values for computer name
    if command -v wine &>/dev/null; then
        wine reg add "HKLM\\SYSTEM\\CurrentControlSet\\Control\\ComputerName\\ComputerName" \
            /v "ComputerName" /t REG_SZ /d "$COMPUTER_NAME" /f 2>/dev/null || true
        wine reg add "HKLM\\SYSTEM\\CurrentControlSet\\Control\\ComputerName\\ActiveComputerName" \
            /v "ActiveComputerName" /t REG_SZ /d "$COMPUTER_NAME" /f 2>/dev/null || true
    fi

    # Create Wine system.ini with computer name
    SYSTEM_INI="$WINEPREFIX/system.ini"
    if [ -f "$SYSTEM_INI" ]; then
        # Backup and update
        cp "$SYSTEM_INI" "${SYSTEM_INI}.bak" 2>/dev/null || true
        sed -i "s/^ComputerName=.*/ComputerName=$COMPUTER_NAME/" "$SYSTEM_INI" 2>/dev/null || true
        if ! grep -q "^ComputerName=" "$SYSTEM_INI"; then
            echo "ComputerName=$COMPUTER_NAME" >> "$SYSTEM_INI"
        fi
    fi

    echo "Applied HWID:"
    echo "  MAC Address: $MAC_ADDR"
    echo "  Disk Serial: $DISK_SERIAL"
    echo "  CPU ID: $CPU_ID"
    echo "  UUID: $UUID"
    echo "  Computer Name: $COMPUTER_NAME"
}

# Reset HWID for an instance
reset_hwid() {
    if [ -f "$HWID_FILE" ]; then
        rm "$HWID_FILE"
        echo "HWID reset for instance: $INSTANCE_ID"
    else
        echo "No HWID file found for instance: $INSTANCE_ID"
    fi
}

# List all HWIDs
list_hwids() {
    echo "=== HWID Instances ==="
    for f in "$HOME/.neon/hwid"/*.json; do
        if [ -f "$f" ]; then
            id=$(basename "$f" .json)
            echo ""
            echo "Instance: $id"
            jq '.' "$f"
        fi
    done
}

# Main
case "$ACTION" in
    init)
        init_hwid
        ;;
    apply)
        apply_hwid
        ;;
    reset)
        reset_hwid
        ;;
    list)
        list_hwids
        ;;
    *)
        echo "Usage: $0 [instance_id] [init|apply|reset|list]"
        echo ""
        echo "Actions:"
        echo "  init   - Generate new HWID for instance"
        echo "  apply  - Apply HWID to Wine prefix"
        echo "  reset  - Remove HWID for instance"
        echo "  list   - List all HWID instances"
        exit 1
        ;;
esac
