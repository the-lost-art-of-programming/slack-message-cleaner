"""Regression tests ensuring Trunk output can run under Firefox MV3 CSP."""

from pathlib import Path
import tempfile
import unittest

from build import externalize_trunk_bootstrap


# ---------------------------------------------------------------------------
# Generated module fixtures
# ---------------------------------------------------------------------------

TRUNK_HTML = """<!doctype html>
<html><head>
<script type="module">
import init, * as bindings from '/cleaner-ui.js';
const wasm = await init({ module_or_path: '/cleaner-ui_bg.wasm' });
window.wasmBindings = bindings;
</script>
</head><body></body></html>"""


# ---------------------------------------------------------------------------
# CSP safety contracts
# ---------------------------------------------------------------------------

class FirefoxBootstrapTests(unittest.TestCase):
    """Keep generated browser code external, local and non-empty."""

    def test_extracts_module_without_inlining_javascript(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            (directory / "index.html").write_text(TRUNK_HTML, encoding="utf-8")

            externalize_trunk_bootstrap(directory)

            html = (directory / "index.html").read_text(encoding="utf-8")
            bootstrap = (directory / "trunk-bootstrap.js").read_text(encoding="utf-8")

            self.assertIn('<script type="module" src="/trunk-bootstrap.js"></script>', html)
            self.assertNotIn("import init", html)
            self.assertIn("await init", bootstrap)

    def test_rejects_missing_generated_bootstrap(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            (directory / "index.html").write_text("<html><body></body></html>")

            with self.assertRaisesRegex(ValueError, "exactly one"):
                externalize_trunk_bootstrap(directory)

    def test_rejects_additional_inline_scripts(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            (directory / "index.html").write_text(
                TRUNK_HTML.replace("</head>", "<script>unexpected()</script></head>")
            )

            with self.assertRaisesRegex(ValueError, "Inline scripts"):
                externalize_trunk_bootstrap(directory)


if __name__ == "__main__":
    unittest.main()
