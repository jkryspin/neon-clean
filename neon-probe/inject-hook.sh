#!/usr/bin/env bash
# neon-probe: macOS hook-based injector helper script
# Usage: ./inject-hook.sh [dll_name]
# Default DLL: probe_opcodes-win.dll
#
# Requires: Wine/MapleStory running, passwordless sudo configured

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INJECTOR="${SCRIPT_DIR}/inject-hook"
DEFAULT_DLL="probe_opcodes-win.dll"
DLL_PATH="Z:\Neon\${1:-$DEFAULT_DLL}"

echo "=== neon-probe macOS Hook-Based Injector ==="
echo "Injector: ${INJECTOR}"
echo "DLL path: ${DLL_PATH}"
echo ""

# Check injector exists
if [[ ! -x "${INJECTOR}" ]]; then
    echo "Error: hook injector not found at ${INJECTOR}"
    echo "Build it first: cd neon-probe && make inject-hook"
    exit 1
fi

# Find Wine process
echo "[1/2] Finding Wine process..."
WINE_PIDS=$(pgrep -f 'winewrapper' 2>/dev/null || true)

if [[ -z "${WINE_PIDS}" ]]; then
    echo "Error: no Wine process found"
    echo "  Is MapleStory running? Start it via Launcher.app first."
    echo ""
    echo "  Manual: pgrep -f winewrapper"
    exit 1
fi

# Pick the first (usually the main winewrapper.exe)
PID=$(echo "${WINE_PIDS}" | head -1)
echo "  Found: PID ${PID}"

# Show process info
echo ""
echo "[2/2] Running hook injector..."
echo "  Command: sudo ${INJECTOR} ${PID} \"${DLL_PATH}\""
echo ""

# Run injector with sudo
sudo "${INJECTOR}" "${PID}" "${DLL_PATH}"

EXIT_CODE=$?
echo ""

if [[ ${EXIT_CODE} -eq 0 ]]; then
    echo "=== Hook installed successfully ==="
    echo "The hook will trigger the next time Wine calls LdrLoadDll."
else
    echo "=== Hook installation failed (exit code: ${EXIT_CODE}) ==="
    echo "Check the output above for error details."
fi

exit ${EXIT_CODE}
