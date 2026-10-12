#!/usr/bin/env python3
"""Headless Firefox package/UI smoke test; requires Firefox and geckodriver."""
from __future__ import annotations

import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
BUNDLE = ROOT / "own-message-cleaner-firefox.zip"
EXTENSION_ID = "own-message-cleaner@local.example"


def uuid_for(profile: Path) -> str | None:
    """Read the internal moz-extension UUID from Firefox's local profile prefs."""
    for filename in ("prefs.js", "user.js"):
        path = profile / filename
        if not path.exists():
            continue
        for line in path.read_text(errors="replace").splitlines():
            if not line.startswith('user_pref("extensions.webextensions.uuids", '):
                continue
            match = re.search(r',\s*("(?:\\.|[^"\\])*")\);', line)
            if match is None:
                continue
            encoded = json.loads(match.group(1))
            return json.loads(encoded).get(EXTENSION_ID)
    return None


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
    with webdriver.Firefox(options=options) as driver:
        installed = driver.install_addon(str(BUNDLE), temporary=True)
        if installed != EXTENSION_ID:
            raise AssertionError(f"Firefox did not install the expected extension: {installed}")
        print("PASS Firefox installed the extension in headless mode")
        profile_path = driver.capabilities.get("moz:profile")
        internal_id = uuid_for(Path(profile_path)) if profile_path else None
        if not internal_id:
            raise AssertionError("Unable to resolve extension's internal UUID for the UI smoke test")
        driver.get(f"moz-extension://{internal_id}/index.html")
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
