#!/usr/bin/env python3
"""Headless Firefox package/UI smoke test; requires Firefox and geckodriver."""
from __future__ import annotations

from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
BUNDLE = ROOT / "own-message-cleaner-firefox.zip"
EXTENSION_ID = "own-message-cleaner@local.example"




def main() -> int:
    if not BUNDLE.exists():
        print("No built XPI/ZIP: run python3 scripts/build.py first", file=sys.stderr)
        return 2
    try:
        from selenium import webdriver
        from selenium.webdriver.common.by import By
        from selenium.webdriver.firefox.options import Options
        from selenium.webdriver.firefox.service import Service
        from selenium.webdriver.support.ui import WebDriverWait
    except ImportError:
        print("Install selenium and geckodriver to perform headless Firefox testing", file=sys.stderr)
        return 2
    options = Options()
    options.add_argument("-headless")
    # Firefox 153+ prohibits WebDriver navigating directly into privileged
    # extension pages. Limit elevated access to the throwaway CI driver.
    service = Service(service_args=["--allow-system-access"])
    with webdriver.Firefox(options=options, service=service) as driver:
        installed = driver.install_addon(str(BUNDLE), temporary=True)
        if installed != EXTENSION_ID:
            raise AssertionError(f"Firefox did not install the expected extension: {installed}")
        print("PASS Firefox installed the extension in headless mode")
        # Firefox 153+ refuses a content-context navigate to moz-extension://.
        # Create the tab as the browser UI, then return to an ordinary content
        # context to assert the extension's real Rust/WASM interface mounts.
        before = set(driver.window_handles)
        with driver.context(driver.CONTEXT_CHROME):
            # Get the installed add-on's *actual* extension URL. An optional
            # test-profile UUID preference is not a reliable source of truth.
            extension_url = driver.execute_script(
                """
                const policy = WebExtensionPolicy.getByID(arguments[0]);
                return policy ? policy.getURL("index.html") : null;
                """,
                EXTENSION_ID,
            )
            if not extension_url or not extension_url.startswith("moz-extension://"):
                raise AssertionError("Firefox could not resolve the installed extension page")
            driver.execute_script(
                """
                gBrowser.selectedTab = gBrowser.addTab(arguments[0], {
                    triggeringPrincipal: Services.scriptSecurityManager.getSystemPrincipal(),
                });
                """,
                extension_url,
            )
        new_tab = WebDriverWait(driver, 15).until(
            lambda browser: next(
                (handle for handle in browser.window_handles if handle not in before),
                None,
            )
        )
        driver.switch_to.window(new_tab)
        try:
            heading = WebDriverWait(driver, 30).until(
                lambda browser: browser.find_element(By.CSS_SELECTOR, "h1")
            )
        except Exception:
            # Diagnosing a broken bootstrap requires knowing whether the page
            # exists before investigating JavaScript or WebAssembly startup.
            print(f"Extension page: {driver.current_url}", file=sys.stderr)
            print(f"Page title: {driver.title}", file=sys.stderr)
            print(f"Page HTML: {driver.page_source[:3000]}", file=sys.stderr)
            raise
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
