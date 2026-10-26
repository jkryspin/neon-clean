#!/usr/bin/env bash
# Spin up a new bot instance with HWID spoofing
# Usage: spinup-bot.sh [instance_id] [channel] [--supervise]
#
# This script:
# 1. Initializes HWID for the instance (if not exists)
# 2. Applies HWID to Wine prefix
# 3. Launches MapleStory Classic World
# 4. Starts the bot pipeline

set -e

cd "$(dirname "$0")/.."
ROOT=$PWD

INSTANCE="${1:-bot-$(date +%s)}"
CHANNEL="${2:-40}"
SUPERVISE=""
for arg in "${@:3}"; do
    case "$arg" in
        --supervise) SUPERVISE="--supervise";;
    esac
done

echo "=========================================="
echo "Spinning up bot instance: $INSTANCE"
echo "Channel: $CHANNEL"
echo "=========================================="

# Step 1: Initialize HWID if not exists
echo ""
echo "[1/4] Checking HWID for instance..."
HWID_FILE="$HOME/.neon/hwid/${INSTANCE}.json"
if [ ! -f "$HWID_FILE" ]; then
    echo "  HWID not found, initializing..."
    "$ROOT/scripts/hwid-spoof-advanced.sh" "$INSTANCE" init
else
    echo "  HWID already exists"
fi

# Step 2: Apply HWID
echo ""
echo "[2/4] Applying HWID..."
"$ROOT/scripts/hwid-spoof-advanced.sh" "$INSTANCE" apply

# Step 3: Launch game
echo ""
echo "[3/4] Launching MapleStory Classic World..."
"$ROOT/scripts/launch-game.sh" "$CHANNEL" "$INSTANCE" 900

# Step 4: Start bot pipeline
echo ""
echo "[4/4] Starting bot pipeline..."
echo ""
echo "Bot is now running with session: $INSTANCE"
echo "To stop: kill -TERM -- -\$(cat run/$INSTANCE.pid)"
echo "To monitor: tail -f logs/$INSTANCE.log"
echo ""

# Start the bot in background if not --foreground
if [ -z "$SUPERVISE" ]; then
    echo "Starting bot pipeline (press Ctrl+C to stop)..."
    "$ROOT/scripts/run-bot.sh" --session "$INSTANCE" --instance "$INSTANCE" "$@"
else
    echo "Starting bot pipeline with supervision..."
    "$ROOT/scripts/run-bot.sh" --session "$INSTANCE" --instance "$INSTANCE" $SUPERVISE "$@"
fi
