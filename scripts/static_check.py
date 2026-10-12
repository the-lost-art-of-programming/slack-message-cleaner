#!/usr/bin/env python3
"""Offline structural/security checks; not a substitute for Cargo/Firefox tests."""
from pathlib import Path
import json
import re
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]


class PackagingChecks(unittest.TestCase):
    """Validate properties that can be proven without a compiler."""

    def test_manifest_uses_firefox_sidebar_and_scoped_permissions(self):
        manifest = json.loads((ROOT / "extension/manifest.json").read_text())
        self.assertEqual(manifest["manifest_version"], 3)
        self.assertEqual(manifest["sidebar_action"]["default_panel"], "index.html")
        self.assertEqual(set(manifest["permissions"]), {"storage", "webRequest"})
        self.assertTrue(all("slack.com" in host for host in manifest["host_permissions"]))
        self.assertNotIn("<all_urls>", manifest["host_permissions"])
        self.assertIn("wasm-unsafe-eval", manifest["content_security_policy"]["extension_pages"])

    def test_no_handwritten_javascript_or_typescript(self):
        suffixes = {".js", ".jsx", ".ts", ".tsx", ".mjs", ".cjs"}
        unexpected = [str(p.relative_to(ROOT)) for p in ROOT.rglob("*") if p.is_file() and p.suffix in suffixes]
        self.assertEqual(unexpected, [], "No JS/TS is authored in the source tree")

    def test_rust_workspace_and_dependency_manifests_parse(self):
        for path in [ROOT / "Cargo.toml", ROOT / "crates/cleaner-core/Cargo.toml",
                     ROOT / "crates/cleaner-ui/Cargo.toml", ROOT / "extension/Trunk.toml"]:
            with self.subTest(path=path):
                with path.open("rb") as source:
                    self.assertIsInstance(tomllib.load(source), dict)

    def test_deletion_requires_selection_checkpoint_and_manual_resume(self):
        core = (ROOT / "crates/cleaner-core/src/queue.rs").read_text()
        runner = (ROOT / "crates/cleaner-ui/src/delete.rs").read_text()
        self.assertIn("self.running = false", core)
        self.assertIn("queue.resume(&account)", runner)
        self.assertIn("queue.start(&key)", runner)
        self.assertLess(runner.index("queue.start(&key)"), runner.index("let outcome = attempt"))
        self.assertIn("checkpoint(&data, state).await?;", runner)
        self.assertRegex(runner, r'\.call\(\s*"conversations\.replies"')
        self.assertRegex(runner, r'\.call\(\s*"chat\.delete"')

    def test_scope_complete_required_for_container_wide_selection(self):
        ui = (ROOT / "crates/cleaner-ui/src/main.rs").read_text()
        scan = (ROOT / "crates/cleaner-ui/src/scan.rs").read_text()
        self.assertIn('current.conversation_complete(&channel)', ui)
        self.assertIn('current.thread_complete(&channel, &root)', ui)
        self.assertIn('current.scan_complete', ui)
        self.assertIn('"conversations.replies"', scan)
        self.assertIn('Slack returned a repeated message cursor', scan)
        self.assertIn('Message pagination stopped without a cursor', scan)

    def test_navigation_uses_slack_permalink_and_reuses_existing_tab(self):
        main = (ROOT / "crates/cleaner-ui/src/main.rs").read_text()
        browser = (ROOT / "crates/cleaner-ui/src/browser.rs").read_text()
        links = (ROOT / "crates/cleaner-core/src/links.rs").read_text()
        self.assertIn('chat.getPermalink', main)
        self.assertIn('permalink_is_for', main)
        self.assertIn('"query"', browser)
        self.assertIn('"update"', browser)
        self.assertIn('"create"', browser)
        self.assertIn('host.ends_with(".slack.com")', links)

    def test_tokens_are_never_saved_to_local_storage(self):
        browser = (ROOT / "crates/cleaner-ui/src/browser.rs").read_text()
        self.assertIn('storage("session")?', browser)
        self.assertIn('"slack_session_token"', browser)
        self.assertIn('storage("local")?', browser)
        self.assertNotIn('save_local("slack_session_token"', browser)
        self.assertNotIn("chrome.storage.local", browser)


if __name__ == "__main__":
    unittest.main(verbosity=2)
