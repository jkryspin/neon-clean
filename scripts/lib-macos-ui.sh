#!/usr/bin/env bash
# macOS UI automation helpers for Wine/CrossOver windows.
# Source this file; do not run it directly.
# Requires: cliclick (brew install cliclick), swift, screencapture

# Find a Wine window by name (returns "WID X Y W H" or empty)
macos_find_window() {
    local target_name="$1"
    swift -e '
import CoreGraphics
let target = CommandLine.arguments[1]
let windows = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]] ?? []
for w in windows {
    let name = w["kCGWindowName"] as? String ?? ""
    if name == target {
        let wid = w["kCGWindowNumber"] as? Int ?? 0
        let b = w["kCGWindowBounds"] as? [String: Any] ?? [:]
        let x = b["X"] as? Double ?? 0
        let y = b["Y"] as? Double ?? 0
        let width = b["Width"] as? Double ?? 0
        let height = b["Height"] as? Double ?? 0
        print("\(wid) \(Int(x)) \(Int(y)) \(Int(width)) \(Int(height))")
        break
    }
}
' "$target_name" 2>/dev/null
}

# Find the Nexon Launcher main window (1024x720)
macos_find_launcher() {
    swift -e '
import CoreGraphics
let windows = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]] ?? []
for w in windows {
    let owner = w["kCGWindowOwnerName"] as? String ?? ""
    let name = w["kCGWindowName"] as? String ?? ""
    let b = w["kCGWindowBounds"] as? [String: Any] ?? [:]
    let width = b["Width"] as? Int ?? 0
    let height = b["Height"] as? Int ?? 0
    if (owner == "Nexon Launcher" || owner == "MapleStory Launcher") && width > 900 && height > 600 {
        let wid = w["kCGWindowNumber"] as? Int ?? 0
        let x = b["X"] as? Double ?? 0
        let y = b["Y"] as? Double ?? 0
        print("\(wid) \(Int(x)) \(Int(y)) \(width) \(height)")
        break
    }
}
' 2>/dev/null
}

# Find the MapleStory game window
macos_find_game_window() {
    swift -e '
import CoreGraphics
let windows = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]] ?? []
for w in windows {
    let owner = w["kCGWindowOwnerName"] as? String ?? ""
    let name = w["kCGWindowName"] as? String ?? ""
    let b = w["kCGWindowBounds"] as? [String: Any] ?? [:]
    let width = b["Width"] as? Int ?? 0
    let height = b["Height"] as? Int ?? 0
    if (name == "MapleStory" || name == "MapleStory Classic World") && width > 800 && height > 500 {
        let wid = w["kCGWindowNumber"] as? Int ?? 0
        let x = b["X"] as? Double ?? 0
        let y = b["Y"] as? Double ?? 0
        print("\(wid) \(Int(x)) \(Int(y)) \(width) \(height)")
        break
    }
}
' 2>/dev/null
}

# Find "What's New" popup (802x526)
macos_find_popup() {
    swift -e '
import CoreGraphics
let windows = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]] ?? []
for w in windows {
    let owner = w["kCGWindowOwnerName"] as? String ?? ""
    let b = w["kCGWindowBounds"] as? [String: Any] ?? [:]
    let width = b["Width"] as? Int ?? 0
    let height = b["Height"] as? Int ?? 0
    if (owner == "Nexon Launcher" || owner == "MapleStory Launcher") && width > 750 && width < 850 && height > 480 && height < 570 {
        let wid = w["kCGWindowNumber"] as? Int ?? 0
        let x = b["X"] as? Double ?? 0
        let y = b["Y"] as? Double ?? 0
        print("\(wid) \(Int(x)) \(Int(y)) \(width) \(height)")
        break
    }
}
' 2>/dev/null
}

# Click at window-relative coordinates (x,y relative to the window's top-left).
# Brings the owning process ("Nexon Launcher") to front first — Wine windows
# ignore clicks while they're behind another window.
macos_click_window() {
    local win_info="$1"  # "WID X Y W H"
    local rel_x="$2"
    local rel_y="$3"

    local win_x win_y
    win_x=$(echo "$win_info" | awk '{print $2}')
    win_y=$(echo "$win_info" | awk '{print $3}')

    local abs_x=$(( win_x + rel_x ))
    local abs_y=$(( win_y + rel_y ))

    osascript -e 'tell application "System Events" to set frontmost of process "MapleStory Launcher" to true' -e 'tell application "System Events" to set frontmost of process "Nexon Launcher" to true' 2>/dev/null
    sleep 0.3
    # Wine/CrossOver windows need mousedown+mouseup; instant click often doesn't register
    cliclick dd:"$abs_x","$abs_y"
    sleep 0.1
    cliclick du:"$abs_x","$abs_y"
}

# Capture a window to a file using screencapture -l <wid>
macos_capture_window() {
    local win_info="$1"  # "WID X Y W H"
    local outfile="$2"

    local wid
    wid=$(echo "$win_info" | awk '{print $1}')

    screencapture -l "$wid" -x "$outfile" 2>/dev/null
}

# Check if an image region is "bright" (above a threshold)
# Uses sips (built-in) to get dimensions, and python3 for pixel analysis
macos_region_brightness() {
    local img="$1" x1="$2" y1="$3" x2="$4" y2="$5"
    python3 -c "
from PIL import Image
im = Image.open('$img')
region = im.crop(($x1, $y1, $x2, $y2)).convert('L')
pixels = list(region.getdata())
mean = sum(pixels) / len(pixels) if pixels else 0
print(int(mean))
" 2>/dev/null || echo "0"
}

# Check if the launcher process is running
macos_launcher_running() {
    pgrep -f "nexon_client.exe" >/dev/null 2>&1 || pgrep -f "nexon_launcher.exe" >/dev/null 2>&1
}

# Start the launcher app
macos_start_launcher() {
    open -a "MapleStory Launcher" 2>/dev/null || open "/Applications/MapleStory Launcher.app" 2>/dev/null
}

# Check if MapleStory game process is running (CrossOver runs it as a native macOS app)
# NOTE: the bundle also keeps a resident helper called "MapleStory Classic World.app";
# match the wine process ("MapleStory.exe") so the helper is not mistaken for the game.
macos_game_pid() {
    pgrep -f "MapleStory.exe" 2>/dev/null | head -1
}
