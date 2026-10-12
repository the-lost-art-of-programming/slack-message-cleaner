#!/usr/bin/env python3
"""Build Rust/WASM sidebar and package an unpacked Firefox WebExtension."""
from pathlib import Path
import json
import shutil
import subprocess
import sys
import zipfile

ROOT = Path(__file__).resolve().parents[1]
DIST = ROOT / "dist"
EXTENSION = ROOT / "extension"
ARCHIVE = ROOT / "own-message-cleaner-firefox.zip"


def main() -> int:
    if shutil.which("cargo") is None or shutil.which("trunk") is None:
        print("Missing Rust/Cargo or Trunk; install prerequisites in docs/BUILD_AND_TEST.md", file=sys.stderr)
        return 2
    subprocess.run(["cargo", "test", "-p", "cleaner-core"], check=True, cwd=ROOT)
    command = ["trunk", "build", "--release"]
    if "--enable-live-delete" in sys.argv[1:]:
        command += ["--features", "live-delete"]
    subprocess.run(command, check=True, cwd=EXTENSION)
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
