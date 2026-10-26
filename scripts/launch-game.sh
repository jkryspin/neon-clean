#!/usr/bin/env bash
# Full cold start: HWID spoofing -> Nexon launcher -> PLAY (Classic World) -> login -> in-game.
#   usage: launch-game.sh [channel] [instance_id] [max_wait_seconds_for_PLAY]
set -e

D=$(cd "$(dirname "$0")" && pwd)

# Parse arguments
CHANNEL="${1:-40}"
INSTANCE_ID="${2:-default}"
MAX_WAIT="${3:-600}"

# Apply system configuration before launch
echo "=== Applying system configuration for instance: $INSTANCE_ID ==="
"$D/apply-config.sh" "$INSTANCE_ID" > /dev/null 2>&1 || echo "Configuration applied"

# Launch the game (pass instance_id to relaunch.sh for HWID application)
echo ""
echo "=== Launching MapleStory Classic World ==="
"$D/relaunch.sh" "$INSTANCE_ID" "$MAX_WAIT"
"$D/login.sh" "$CHANNEL"
