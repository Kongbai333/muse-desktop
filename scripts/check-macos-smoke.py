#!/usr/bin/env python3
"""Exercise the native application on a macOS runner with disposable accounts."""
import json
import pathlib
import subprocess
import sys
import tempfile


def main():
    if sys.platform != "darwin":
        raise SystemExit("This native smoke check requires macOS")
    binary = pathlib.Path("target/release/muse-desktop").resolve()
    with tempfile.TemporaryDirectory(prefix="muse-native-smoke-") as directory:
        root = pathlib.Path(directory)
        try:
            subprocess.run([str(binary), "--smoke-test", "--data-dir", str(root)], check=True, timeout=75)
        except (subprocess.SubprocessError, OSError):
            if (root / "crash.log").exists():
                print((root / "crash.log").read_text(errors="replace"))
            raise
        report = json.loads((root / "smoke-result.json").read_text())
        print(json.dumps(report, indent=2))
        for check in ("passed", "actual_profile_bindings_verified", "independent_account_storage", "same_account_tabs_share_storage"):
            if report.get(check) is not True:
                raise SystemExit(f"Native smoke failed: {check}")
        # Pixel composition is a separate Windows-only test; never imply that
        # DOM/profile checks inspected rendered macOS pixels.
        if report.get("webview_pixels_visible") is not None:
            raise SystemExit("Unexpected Windows pixel result in the macOS smoke report")


if __name__ == "__main__":
    main()
