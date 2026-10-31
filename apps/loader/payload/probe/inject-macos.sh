#!/usr/bin/env bash
# neon-probe: macOS injector helper script
# Usage: ./inject-helpers.sh [dll_name]
# Default DLL: probe_opcodes-win.dll
#
# Requires: Wine/MapleStory running, passwordless sudo configured

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INJECTOR="${SCRIPT_DIR}/inject-macos"
DEFAULT_DLL="probe_opcodes-win.dll"
DLL_PATH="Z:\Neon\${1:-$DEFAULT_DLL}"

echo "=== neon-probe macOS Injector Helper ==="
echo "Injector: ${INJECTOR}"
echo "DLL path: ${DLL_PATH}"
echo ""

# Check injector exists
if [[ ! -x "${INJECTOR}" ]]; then
    echo "Error: injector not found at ${INJECTOR}"
    echo "Build it first: cd neon-probe && make macos"
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
echo "[2/2] Running injector..."
echo "  Command: sudo ${INJECTOR} ${PID} \"${DLL_PATH}\""
echo ""

# Run injector with sudo
sudo "${INJECTOR}" "${PID}" "${DLL_PATH}"

EXIT_CODE=$?
echo ""

if [[ ${EXIT_CODE} -eq 0 ]]; then
    echo "=== Injection completed successfully ==="
else
    echo "=== Injection failed (exit code: ${EXIT_CODE}) ==="
    echo "Check the output above for error details."
fi

exit ${EXIT_CODE}
