#!/usr/bin/env python3
"""
Connect to Firefox via geckodriver and watch signup
"""

import time
from selenium import webdriver
from selenium.webdriver.firefox.options import Options
from selenium.webdriver.common.by import By
import signal
import sys

def log_page_state(driver):
    """Log the current page state"""
    print("\n=== Current Page State ===")
    print(f"URL: {driver.current_url}")

    # Get dropdown values
    toggles = driver.find_elements(By.CLASS_NAME, "dropdown-toggle")
    print("Dropdown values:")
    for i, toggle in enumerate(toggles):
        text = toggle.text.strip()
        has_error = "error" in toggle.get_attribute("class")
        print(f"  {i}: {text} (error: {has_error})")

    # Get help messages
    help_blocks = driver.find_elements(By.CLASS_NAME, "help-block")
    print("Help messages:")
    for block in help_blocks:
        text = block.text.strip()
        visible = block.get_attribute("style") != "display: none;"
        print(f"  - {text} (visible: {visible})")
    print()

def main():
    print("Connecting to Firefox via geckodriver...")

    options = Options()
    # Don't set binary_location for snap Firefox

    try:
        # Connect to existing geckodriver
        driver = webdriver.Remote('http://localhost:4444', options=options)
        print("Connected to Firefox!")
    except Exception as e:
        print(f"Error connecting: {e}")
        print("Starting new Firefox session...")
        driver = webdriver.Firefox(options=options)

    print(f"Current URL: {driver.current_url}")

    # Navigate to signup page if not already there
    if "nexon.com/account" not in driver.current_url:
        print("Navigating to signup page...")
        driver.get("https://www.nexon.com/account/en/create")
        time.sleep(2)

    # Log initial state
    log_page_state(driver)

    print("\n=== Monitoring page changes ===")
    print("Please interact with the page. The script will log changes every 2 seconds.\n")
    print("Press Ctrl+C to stop.\n")

    # Set up signal handler for clean exit
    def signal_handler(sig, frame):
        print("\nStopping...")
        driver.quit()
        sys.exit(0)

    signal.signal(signal.SIGINT, signal_handler)

    # Monitor page changes
    last_url = driver.current_url
    last_html = driver.page_source

    try:
        while True:
            time.sleep(2)

            # Check for URL change
            current_url = driver.current_url
            if current_url != last_url:
                print(f"\n[URL CHANGE] {current_url}\n")
                last_url = current_url
                log_page_state(driver)

            # Check for HTML change
            current_html = driver.page_source
            if current_html != last_html:
                print("[HTML CHANGE DETECTED]")
                log_page_state(driver)
                last_html = current_html

    except KeyboardInterrupt:
        print("\nStopping...")
        driver.quit()

if __name__ == "__main__":
    main()
