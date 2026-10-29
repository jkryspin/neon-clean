#!/usr/bin/env bash
# Advanced HWID Spoofing for MapleStory Classic World
# Cross-platform: macOS (CrossOver) and Linux (Wine)
# Usage: hwid-spoof-advanced.sh [instance_id] [action]

set -e

# Create directories early
HWID_DIR="$HOME/.neon/hwid"
mkdir -p "$HWID_DIR"

# Detect platform and set Wine paths
case "$(uname -s)" in
    Darwin)
        PLATFORM="macos"
        MAPLE_APP="/Applications/MapleStory Launcher.app"
        WINE_BIN="$MAPLE_APP/Contents/SharedSupport/maplestoryna/MapleStory Launcher/wine"
        WINEPREFIX="${WINEPREFIX:-$HOME/Library/Application Support/MapleStoryNA/Bottles/maplestory}"
        if [ ! -x "$WINE_BIN" ]; then
            echo "ERROR: MapleStory Launcher.app not found or Wine binary missing."
            echo "Expected: $WINE_BIN"
            exit 1
        fi
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

# Generate unique hardware identifiers
generate_hwid() {
    local uuid
    uuid=$(uuidgen 2>/dev/null | tr '[:upper:]' '[:lower:]' || python3 -c "import uuid; print(uuid.uuid4())")
    local machine_guid
    machine_guid=$(uuidgen 2>/dev/null | tr '[:lower:]' '[:upper:]' || python3 -c "import uuid; print(str(uuid.uuid4()).upper())")
    local mac=$(printf '%02x:%02x:%02x:%02x:%02x:%02x' $((0x02 + RANDOM % 254)) $((RANDOM % 256)) $((RANDOM % 256)) $((RANDOM % 256)) $((RANDOM % 256)) $((RANDOM % 256)))
    local mac_bare=$(echo "$mac" | tr -d ':')
    local disk_serial=$(printf 'S%08d%04d%04d' $((RANDOM * RANDOM % 100000000)) $((RANDOM % 10000)) $((RANDOM % 10000)))
    local cpu_id=$(printf '%08X-%04X-%04X-%04X-%012X' $((RANDOM * RANDOM % 4294967296)) $((RANDOM % 65536)) $((RANDOM % 65536)) $((RANDOM % 65536)) $((RANDOM * RANDOM % 281474976710656)))
    local motherboard_serial=$(printf 'MB-%04X-%04X-%04X' $((RANDOM % 65536)) $((RANDOM % 65536)) $((RANDOM % 65536)))
    local volume_serial=$(printf '%04X-%04X' $((RANDOM % 65536)) $((RANDOM % 65536)))
    local product_id=$(printf '%05d-OEM-%07d-%05d' $((RANDOM % 100000)) $((RANDOM * RANDOM % 10000000)) $((RANDOM % 100000)))
    local hw_profile_guid
    hw_profile_guid="{$(uuidgen 2>/dev/null | tr '[:lower:]' '[:upper:]' || python3 -c "import uuid; print(str(uuid.uuid4()).upper())")}"
    local bios_vendor="American Megatrends Inc."
    local system_manufacturer="System manufacturer"
    local system_product="System Product Name"
    local created
    created=$(date -Iseconds 2>/dev/null || date -u +%Y-%m-%dT%H:%M:%S%z)

    cat << EOF
{
    "instance_id": "$INSTANCE_ID",
    "created": "$created",
    "hardware": {
        "uuid": "$uuid",
        "machine_guid": "$machine_guid",
        "mac_address": "$mac",
        "mac_bare": "$mac_bare",
        "disk_serial": "$disk_serial",
        "volume_serial": "$volume_serial",
        "cpu_id": "$cpu_id",
        "motherboard_serial": "$motherboard_serial",
        "product_id": "$product_id",
        "hw_profile_guid": "$hw_profile_guid",
        "bios_vendor": "$bios_vendor",
        "system_manufacturer": "$system_manufacturer",
        "system_product": "$system_product"
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

# Portable sed -i (BSD vs GNU)
sed_i() {
    if [ "$PLATFORM" = "macos" ]; then
        sed -i '' "$@"
    else
        sed -i "$@"
    fi
}

# Helper: set a registry value (swallows errors for optional keys)
reg_set() {
    local key="$1" name="$2" type="$3" data="$4"
    "$WINE_BIN" reg add "$key" /v "$name" /t "$type" /d "$data" /f 2>/dev/null || true
}

# Helper: find a Wine ini file and ensure it's a real file (not a symlink,
# which breaks BSD sed -i on macOS; CrossOver bottles symlink ini files
# to shared BuiltinBottles templates)
find_ini() {
    local name="$1"
    local path="$WINEPREFIX/drive_c/windows/$name"
    [ -f "$path" ] || return 1
    if [ -L "$path" ]; then
        local target
        target=$(readlink "$path")
        rm "$path"
        cp "$target" "$path"
    fi
    echo "$path"
}

# Apply HWID using Wine registry and system files
apply_hwid() {
    if [ ! -f "$HWID_FILE" ]; then
        echo "HWID file not found. Run: hwid-spoof-advanced.sh $INSTANCE_ID init"
        exit 1
    fi

    echo "=== Applying HWID for instance: $INSTANCE_ID ($PLATFORM) ==="

    # Read all values from the HWID file
    UUID=$(jq -r '.hardware.uuid' "$HWID_FILE")
    MACHINE_GUID=$(jq -r '.hardware.machine_guid' "$HWID_FILE")
    MAC_ADDR=$(jq -r '.hardware.mac_address' "$HWID_FILE")
    MAC_BARE=$(jq -r '.hardware.mac_bare // empty' "$HWID_FILE")
    [ -z "$MAC_BARE" ] && MAC_BARE=$(echo "$MAC_ADDR" | tr -d ':')
    DISK_SERIAL=$(jq -r '.hardware.disk_serial' "$HWID_FILE")
    VOLUME_SERIAL=$(jq -r '.hardware.volume_serial // empty' "$HWID_FILE")
    CPU_ID=$(jq -r '.hardware.cpu_id' "$HWID_FILE")
    MOTHERBOARD_SERIAL=$(jq -r '.hardware.motherboard_serial' "$HWID_FILE")
    PRODUCT_ID=$(jq -r '.hardware.product_id // empty' "$HWID_FILE")
    HW_PROFILE_GUID=$(jq -r '.hardware.hw_profile_guid // empty' "$HWID_FILE")
    BIOS_VENDOR=$(jq -r '.hardware.bios_vendor // "American Megatrends Inc."' "$HWID_FILE")
    SYSTEM_MFG=$(jq -r '.hardware.system_manufacturer // "System manufacturer"' "$HWID_FILE")
    SYSTEM_PRODUCT=$(jq -r '.hardware.system_product // "System Product Name"' "$HWID_FILE")
    COMPUTER_NAME=$(jq -r '.wine.computer_name' "$HWID_FILE")

    # --- 1. Computer Name ---
    echo "[1/7] Computer name..."
    reg_set "HKLM\\SYSTEM\\CurrentControlSet\\Control\\ComputerName\\ComputerName" "ComputerName" REG_SZ "$COMPUTER_NAME"
    reg_set "HKLM\\SYSTEM\\CurrentControlSet\\Control\\ComputerName\\ActiveComputerName" "ActiveComputerName" REG_SZ "$COMPUTER_NAME"

    # --- 2. MachineGuid (primary HWID fingerprint) ---
    echo "[2/7] MachineGuid..."
    if [ -n "$MACHINE_GUID" ] && [ "$MACHINE_GUID" != "null" ]; then
        reg_set "HKLM\\SOFTWARE\\Microsoft\\Cryptography" "MachineGuid" REG_SZ "$MACHINE_GUID"
    fi

    # --- 3. MAC Address (network adapter) ---
    echo "[3/7] MAC address..."
    if [ -n "$MAC_BARE" ]; then
        reg_set "HKLM\\SYSTEM\\CurrentControlSet\\Control\\Class\\{4d36e972-e325-11ce-bfc1-08002be10318}\\0000" "NetworkAddress" REG_SZ "$MAC_BARE"
        reg_set "HKLM\\SYSTEM\\CurrentControlSet\\Control\\Class\\{4d36e972-e325-11ce-bfc1-08002be10318}\\0001" "NetworkAddress" REG_SZ "$MAC_BARE"
    fi

    # --- 4. Disk / Volume serial ---
    echo "[4/7] Disk serial & volume serial..."
    reg_set "HKLM\\HARDWARE\\DEVICEMAP\\Scsi\\Scsi Port 0\\Scsi Bus 0\\Target Id 0\\Logical Unit Id 0" "Identifier" REG_SZ "$DISK_SERIAL"
    reg_set "HKLM\\HARDWARE\\DEVICEMAP\\Scsi\\Scsi Port 0\\Scsi Bus 0\\Target Id 0\\Logical Unit Id 0" "SerialNumber" REG_SZ "$DISK_SERIAL"
    if [ -n "$VOLUME_SERIAL" ] && [ "$VOLUME_SERIAL" != "null" ]; then
        local vol_file="$WINEPREFIX/drive_c/.windows-serial"
        echo "$VOLUME_SERIAL" > "$vol_file" 2>/dev/null || true
    fi

    # --- 5. ProductId & HW Profile GUID ---
    echo "[5/7] ProductId & HW profile..."
    if [ -n "$PRODUCT_ID" ] && [ "$PRODUCT_ID" != "null" ]; then
        reg_set "HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion" "ProductId" REG_SZ "$PRODUCT_ID"
    fi
    if [ -n "$HW_PROFILE_GUID" ] && [ "$HW_PROFILE_GUID" != "null" ]; then
        reg_set "HKLM\\SYSTEM\\CurrentControlSet\\Control\\IDConfigDB\\Hardware Profiles\\0001" "HwProfileGuid" REG_SZ "$HW_PROFILE_GUID"
    fi

    # --- 6. SMBIOS / System Information ---
    echo "[6/7] SMBIOS identifiers..."
    local SYSINFO_KEY="HKLM\\SYSTEM\\CurrentControlSet\\Control\\SystemInformation"
    reg_set "$SYSINFO_KEY" "ComputerHardwareId" REG_SZ "{$UUID}"
    reg_set "$SYSINFO_KEY" "BIOSVersion" REG_SZ "$BIOS_VENDOR"
    reg_set "$SYSINFO_KEY" "SystemManufacturer" REG_SZ "$SYSTEM_MFG"
    reg_set "$SYSINFO_KEY" "SystemProductName" REG_SZ "$SYSTEM_PRODUCT"
    reg_set "$SYSINFO_KEY" "BIOSReleaseDate" REG_SZ "$(printf '%02d/%02d/20%02d' $((RANDOM % 12 + 1)) $((RANDOM % 28 + 1)) $((RANDOM % 5 + 20)))"
    reg_set "$SYSINFO_KEY" "BaseBoardManufacturer" REG_SZ "ASUSTeK COMPUTER INC."
    reg_set "$SYSINFO_KEY" "BaseBoardProduct" REG_SZ "$MOTHERBOARD_SERIAL"

    # --- 7. Update system.ini and win.ini ---
    echo "[7/7] INI files..."
    local SYSTEM_INI=$(find_ini "system.ini")
    if [ -n "$SYSTEM_INI" ]; then
        cp "$SYSTEM_INI" "${SYSTEM_INI}.hwid.bak"
        if grep -q "\[computername\]" "$SYSTEM_INI"; then
            sed_i 's/^ComputerName=.*/ComputerName='"$COMPUTER_NAME"'/' "$SYSTEM_INI"
        else
            printf '\n[computername]\nComputerName=%s\n' "$COMPUTER_NAME" >> "$SYSTEM_INI"
        fi
    fi

    local WIN_INI=$(find_ini "win.ini")
    if [ -n "$WIN_INI" ]; then
        cp "$WIN_INI" "${WIN_INI}.hwid.bak"
        if grep -q "\[windows\]" "$WIN_INI"; then
            sed_i 's/^computer=.*/computer='"$COMPUTER_NAME"'/' "$WIN_INI"
        else
            printf '\n[windows]\ncomputer=%s\n' "$COMPUTER_NAME" >> "$WIN_INI"
        fi
    fi

    export WINE_COMPUTER_NAME="$COMPUTER_NAME"

    echo ""
    echo "=== HWID Applied ==="
    echo "Platform:       $PLATFORM"
    echo "Wine prefix:    $WINEPREFIX"
    echo "Computer Name:  $COMPUTER_NAME"
    echo "MachineGuid:    $MACHINE_GUID"
    echo "MAC Address:    $MAC_ADDR"
    echo "Disk Serial:    $DISK_SERIAL"
    echo "Volume Serial:  ${VOLUME_SERIAL:-n/a}"
    echo "ProductId:      ${PRODUCT_ID:-n/a}"
    echo "HW Profile:     ${HW_PROFILE_GUID:-n/a}"
    echo "SMBIOS UUID:    $UUID"
}

# Apply HWID and launch game
apply_and_launch() {
    apply_hwid
    echo ""
    echo "Launching MapleStory..."
    "$(dirname "$0")/launch-game.sh" "$@"
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
    echo "Platform: $(uname -s)"
    echo "Wine: $WINE_BIN"
    echo "Prefix: $WINEPREFIX"
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
