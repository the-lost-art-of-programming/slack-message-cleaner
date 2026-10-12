#!/usr/bin/env python3
"""Build Rust/WASM sidebar and package an unpacked Firefox WebExtension."""
from pathlib import Path
import json
import re
import shutil
import subprocess
import sys
import zipfile

ROOT = Path(__file__).resolve().parents[1]
DIST = ROOT / "dist"
EXTENSION = ROOT / "extension"
ARCHIVE = ROOT / "own-message-cleaner-firefox.zip"


# ---------------------------------------------------------------------------
# Firefox-safe Trunk bootstrap
# ---------------------------------------------------------------------------

# Trunk generates an inline module script by default. Firefox MV3 forbids
# inline extension scripts, even though generated JavaScript files are allowed.
# Move the exact emitted module body to a local file without authoring JS.
_TRUNK_MODULE = re.compile(r'<script\s+type="module"\s*>(.*?)</script>', re.DOTALL)


def externalize_trunk_bootstrap(directory: Path) -> None:
    """Make Trunk's generated module compliant with Firefox extension CSP.

    The transformation fails closed if Trunk's output format changes:
    a silently empty sidebar is worse than a failed build.

    Args:
        directory: The Trunk output directory containing index.html.

    Raises:
        ValueError: If exactly one generated inline module was not found.
    """
    html_path = directory / "index.html"
    html = html_path.read_text(encoding="utf-8")
    matches = list(_TRUNK_MODULE.finditer(html))

    if len(matches) != 1:
        raise ValueError("Expected exactly one Trunk-generated inline module")

    generated_source = matches[0].group(1).strip()
    if "import init" not in generated_source:
        raise ValueError("Unrecognized Trunk-generated WASM bootstrap")

    bootstrap_name = "trunk-bootstrap.js"
    (directory / bootstrap_name).write_text(generated_source + "\n", encoding="utf-8")

    external = f'<script type="module" src="/{bootstrap_name}"></script>'
    updated = html[: matches[0].start()] + external + html[matches[0].end() :]

    # Prevent regressions if Trunk starts emitting additional inline scripts.
    opening_scripts = re.findall(r"<script\b[^>]*>", updated, re.IGNORECASE)
    if any(re.search(r"\bsrc\s*=", tag, re.IGNORECASE) is None for tag in opening_scripts):
        raise ValueError("Inline scripts are forbidden by Firefox MV3 CSP")

    html_path.write_text(updated, encoding="utf-8")


def main() -> int:
    if shutil.which("cargo") is None or shutil.which("trunk") is None:
        print("Missing Rust/Cargo or Trunk; install prerequisites in docs/BUILD_AND_TEST.md", file=sys.stderr)
        return 2
    subprocess.run(["cargo", "test", "-p", "cleaner-core"], check=True, cwd=ROOT)
    command = ["trunk", "build", "--release"]
    if "--enable-live-delete" in sys.argv[1:]:
        command += ["--features", "live-delete"]
    subprocess.run(command, check=True, cwd=EXTENSION)
    externalize_trunk_bootstrap(DIST)
    shutil.copyfile(EXTENSION / "manifest.json", DIST / "manifest.json")
    manifest = json.loads((DIST / "manifest.json").read_text())
    if manifest.get("manifest_version") != 3:
        raise ValueError("Unexpected manifest version")
    if not any(DIST.glob("*.wasm")) or not any(DIST.glob("*.js")):
        raise ValueError("The packaged extension is missing generated JavaScript or WASM")
    with zipfile.ZipFile(ARCHIVE, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
        for path in sorted(DIST.rglob("*")):
            if path.is_file():
                bundle.write(path, path.relative_to(DIST))
    print(f"Built {ARCHIVE}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
