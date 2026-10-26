#!/usr/bin/env python3
"""
Connect to existing Firefox via Remote Debugging and watch signup
"""

import time
import json
import urllib.request
import urllib.parse
import websocket
import threading

def main():
    print("Connecting to Firefox via Remote Debugging (localhost:6000)...")

    # Get the list of tabs using the Firefox debugging API
    try:
        # Firefox debugging API endpoint
        req = urllib.request.Request('http://localhost:6000/about/debugging')
        with urllib.request.urlopen(req, timeout=5) as response:
            data = response.read().decode('utf-8')
            print("Debug page loaded")
    except Exception as e:
        print(f"Error loading debug page: {e}")

    # Try to connect via WebSocket to the debugger
    print("\nConnecting via WebSocket...")

    ws = websocket.create_connection("ws://localhost:6000/debugger")
    print("Connected to WebSocket")

    # Send a message to get the list of pages
    ws.send(json.dumps({
        "method": "Browser.getPages",
        "params": {},
        "id": 1
    }))

    response = ws.recv()
    print(f"Response: {response}")

    # Keep listening for messages
    print("\n=== Monitoring page changes ===\n")
    print("Please interact with the page. The script will log changes.\n")
    print("Press Ctrl+C to stop.\n")

    try:
        while True:
            try:
                message = ws.recv()
                if message:
                    data = json.loads(message)
                    print(f"[WS] {data.get('method', 'unknown')}: {data.get('params', {})}")
            except websocket.WebSocketTimeoutException:
                continue
            except Exception as e:
                print(f"Error: {e}")
                break
    except KeyboardInterrupt:
        print("\nStopping...")
        ws.close()

if __name__ == "__main__":
    main()
