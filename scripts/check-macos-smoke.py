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
    for flag in ("--smoke-test", "--proxy-smoke-test", "--proxy-edit-smoke-test"):
        check_run(binary, flag)


def check_run(binary, flag):
    with tempfile.TemporaryDirectory(prefix="muse-native-smoke-") as directory:
        root = pathlib.Path(directory)
        try:
            subprocess.run([str(binary), flag, "--data-dir", str(root)], check=True, timeout=75)
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
        if flag == "--proxy-smoke-test" and not report.get("native_authenticated_upstream_requests"):
            raise SystemExit("WKWebView did not route through its authenticated proxy")
        if flag == "--proxy-edit-smoke-test":
            for check in ("same_endpoint_credentials_applied", "storage_preserved_after_proxy_change", "system_network_switch_applied"):
                if report.get(check) is not True:
                    raise SystemExit(f"WKWebView proxy edit failed: {check}")


if __name__ == "__main__":
    main()
