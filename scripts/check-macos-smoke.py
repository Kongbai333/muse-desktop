#!/usr/bin/env python3
"""Exercise the native application on a macOS runner with disposable accounts."""
import argparse
import json
import os
import pathlib
import socket
import subprocess
import sys
import tempfile


def main():
    if sys.platform != "darwin":
        raise SystemExit("This native smoke check requires macOS")
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=pathlib.Path, default=pathlib.Path("target/release/muse-desktop"))
    binary = parser.parse_args().binary.resolve()
    flags = ["--smoke-test"]
    host = os.environ.get("MUSE_SMOKE_PROXY_HOST")
    if host:
        if host != "muse-proxy-smoke.test" or socket.gethostbyname(host) != "127.0.0.1":
            raise SystemExit("The CI-only proxy fixture hostname must resolve to loopback")
        flags += ["--proxy-smoke-test", "--proxy-edit-smoke-test"]
    else:
        print("Proxy integration checks require the CI-only hostname fixture; running direct checks only.", flush=True)
    for flag in flags:
        check_run(binary, flag)


def check_run(binary, flag):
    with tempfile.TemporaryDirectory(prefix="muse-native-smoke-") as directory:
        root = pathlib.Path(directory)
        try:
            extra = ["--cookie-import-smoke-test"] if flag in ("--smoke-test", "--proxy-smoke-test") else []
            subprocess.run([str(binary), flag, *extra, "--data-dir", str(root)], check=True, timeout=75)
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
        if flag in ("--smoke-test", "--proxy-smoke-test"):
            for check in ("cookie_import_isolated_and_private", "cookie_import_cookie_sent_on_navigation", "cookie_import_original_account_restored", "cookie_import_proxy_route_preserved", "cookie_import_expected_route_used"):
                if report.get(check) is not True:
                    raise SystemExit(f"Native Cookie import failed: {check}")
        if flag == "--proxy-smoke-test" and not report.get("native_authenticated_upstream_requests"):
            raise SystemExit("WKWebView did not route through its authenticated proxy")
        if flag == "--proxy-edit-smoke-test":
            for check in ("same_endpoint_credentials_applied", "storage_preserved_after_proxy_change", "system_network_switch_applied"):
                if report.get(check) is not True:
                    raise SystemExit(f"WKWebView proxy edit failed: {check}")


if __name__ == "__main__":
    main()
