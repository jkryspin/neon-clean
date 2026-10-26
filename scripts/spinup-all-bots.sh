#!/usr/bin/env bash
# Spin up all 5 bot instances for MapleStory Classic World
# Usage: spinup-all-bots.sh [channel] [--supervise]

set -e

cd "$(dirname "$0")/.."
ROOT=$PWD

CHANNEL="${1:-40}"
SUPERVISE=""
for arg in "${@:2}"; do
    case "$arg" in
        --supervise) SUPERVISE="--supervise";;
    esac
done

NUM_BOTS=5

echo "=========================================="
echo "Spinning up $NUM_BOTS bot instances"
echo "Channel: $CHANNEL"
echo "=========================================="

# Create bot instances
for i in $(seq 1 $NUM_BOTS); do
    INSTANCE="neon-bot-$i"
    echo ""
    echo "[$i/$NUM_BOTS] Setting up $INSTANCE..."

    # Initialize HWID if not exists
    HWID_FILE="$HOME/.neon/hwid/${INSTANCE}.json"
    if [ ! -f "$HWID_FILE" ]; then
        "$ROOT/scripts/hwid-spoof-advanced.sh" "$INSTANCE" init > /dev/null
        echo "  Initialized new HWID"
    else
        echo "  HWID exists, reusing"
    fi

    # Apply HWID
    "$ROOT/scripts/hwid-spoof-advanced.sh" "$INSTANCE" apply > /dev/null
    echo "  Applied HWID"
done

echo ""
echo "=========================================="
echo "Launching all bots..."
echo "=========================================="

# Launch all bots
for i in $(seq 1 $NUM_BOTS); do
    INSTANCE="neon-bot-$i"
    echo "[$i/$NUM_BOTS] Launching $INSTANCE..."

    # Launch game and bot in background
    (
        # Launch game
        "$ROOT/scripts/launch-game.sh" "$CHANNEL" "$INSTANCE" 900

        # Start bot pipeline
        "$ROOT/scripts/run-bot.sh" --session "$INSTANCE" --instance "$INSTANCE" $SUPERVISE 2>&1 | tee "logs/$INSTANCE.log"
    ) &

    sleep 5  # Stagger launches
done

echo ""
echo "=========================================="
echo "All bots launched!"
echo "=========================================="
echo ""
echo "To monitor all bots:"
echo "  tail -f logs/neon-bot-*.log"
echo ""
echo "To stop all bots:"
echo "  for i in 1 2 3 4 5; do kill -TERM -- -\$(cat run/neon-bot-\$i.pid 2>/dev/null) 2>/dev/null; done"
echo ""
echo "To list HWIDs:"
echo "  $ROOT/scripts/hwid-spoof-advanced.sh list"
echo ""
