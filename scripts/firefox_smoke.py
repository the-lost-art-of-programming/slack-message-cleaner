#!/usr/bin/env python3
"""Headless Firefox package/UI smoke test; requires Firefox and geckodriver."""
from __future__ import annotations

import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
BUNDLE = ROOT / "own-message-cleaner-firefox.zip"
EXTENSION_ID = "own-message-cleaner@local.example"


# Pin a UUID before Firefox starts instead of reading a preference file that
# Firefox may not flush until browser shutdown. This applies ONLY to the
# throwaway headless test profile, never to end-user installations.
TEST_UUID = "abf2caa9-d9fe-49eb-94fa-e95efdf583e7"


def main() -> int:
    if not BUNDLE.exists():
        print("No built XPI/ZIP: run python3 scripts/build.py first", file=sys.stderr)
        return 2
    try:
        from selenium import webdriver
        from selenium.webdriver.common.by import By
        from selenium.webdriver.firefox.options import Options
        from selenium.webdriver.support.ui import WebDriverWait
    except ImportError:
        print("Install selenium and geckodriver to perform headless Firefox testing", file=sys.stderr)
        return 2
    options = Options()
    options.add_argument("-headless")
    options.set_preference(
        "extensions.webextensions.uuids",
        json.dumps({EXTENSION_ID: TEST_UUID}),
    )
    with webdriver.Firefox(options=options) as driver:
        installed = driver.install_addon(str(BUNDLE), temporary=True)
        if installed != EXTENSION_ID:
            raise AssertionError(f"Firefox did not install the expected extension: {installed}")
        print("PASS Firefox installed the extension in headless mode")
        # A deterministic UUID avoids a race with prefs.js serialization.
        # Browsing the page verifies the packaged Rust/WASM UI actually mounts.
        driver.get(f"moz-extension://{TEST_UUID}/index.html")
        heading = WebDriverWait(driver, 20).until(lambda browser: browser.find_element(By.CSS_SELECTOR, "h1"))
        if heading.text != "Own Message Cleaner":
            raise AssertionError(f"Unexpected UI heading: {heading.text}")
        connect = driver.find_element(By.XPATH, "//button[contains(., 'Connect / Restore')]")
        if not connect.is_displayed():
            raise AssertionError("Connect button is not visible")
        print("PASS Leptos/WASM sidebar mounted and displayed Connect")
    print("NOTE: This does not test live Slack authentication or deletion")
    return 0


if __name__ == "__main__":
    sys.exit(main())
