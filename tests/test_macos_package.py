import importlib.util
import pathlib
import plistlib
import struct
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("package_macos", ROOT / "scripts/package-macos.py")
packager = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packager)
smoke_spec = importlib.util.spec_from_file_location("macos_smoke", ROOT / "scripts/check-macos-smoke.py")
smoke = importlib.util.module_from_spec(smoke_spec)
smoke_spec.loader.exec_module(smoke)


class MacPackageTests(unittest.TestCase):
    def test_native_cookie_paths_include_the_new_transfer_adapter_check(self):
        self.assertTrue(callable(getattr(smoke, "native_cookie_flags", None)))
        for flag in ("--smoke-test", "--proxy-smoke-test"):
            self.assertEqual(smoke.native_cookie_flags(flag),
                             ["--cookie-import-smoke-test", "--transfer-smoke-test"])
        self.assertEqual(smoke.native_cookie_flags("--proxy-edit-smoke-test"), [])

    def test_native_transfer_report_cannot_be_missing_or_partially_passing(self):
        self.assertTrue(callable(getattr(smoke, "check_transfer_report", None)))
        import json
        with tempfile.TemporaryDirectory(prefix="muse-native-contract-") as directory:
            root = pathlib.Path(directory)
            with self.assertRaises(SystemExit):
                smoke.check_transfer_report(root)
            report = {key: True for key in ("passed", "native_scope_readback", "http_only_same_site_expiry",
                "shared_proxy_snapshot", "account_isolation", "no_navigation_before_commit", "no_metadata_before_commit")}
            report["no_navigation_before_commit"] = False
            (root / "transfer-native-result.json").write_text(json.dumps(report))
            with self.assertRaises(SystemExit):
                smoke.check_transfer_report(root)
            report["no_navigation_before_commit"] = True
            (root / "transfer-native-result.json").write_text(json.dumps(report))
            smoke.check_transfer_report(root)

    def test_bundle_uses_actual_macho_architecture_and_contains_only_public_files(self):
        with tempfile.TemporaryDirectory(prefix="muse-mac-package-") as directory:
            root = pathlib.Path(directory)
            binary = root / "muse-desktop"
            payload = struct.pack("<II", 0xFEEDFACF, 0x0100000C) + b"fixture executable"
            binary.write_bytes(payload)
            (root / "private.keystore").write_text("must never ship")
            app = packager.assemble_bundle(ROOT, binary, root / "bundle", "arm64", "0.1.0")
            self.assertEqual((app / "Contents/MacOS/Muse").read_bytes(), payload)
            metadata = plistlib.loads((app / "Contents/Info.plist").read_bytes())
            self.assertEqual(metadata["CFBundleExecutable"], "Muse")
            self.assertEqual(metadata["LSMinimumSystemVersion"], "14.0")
            self.assertEqual(metadata["CFBundleShortVersionString"], "0.1.0")
            self.assertTrue((app / "Contents/Resources/LICENSE").is_file())
            self.assertFalse(any("keystore" in str(path) for path in app.rglob("*")))

    def test_windows_executable_is_rejected(self):
        with tempfile.TemporaryDirectory(prefix="muse-mac-package-") as directory:
            binary = pathlib.Path(directory) / "client"
            binary.write_bytes(b"MZWindows executable")
            with self.assertRaises(ValueError):
                packager.validate_binary(binary, "arm64")

    def test_wrong_architecture_is_rejected(self):
        with tempfile.TemporaryDirectory(prefix="muse-mac-package-") as directory:
            binary = pathlib.Path(directory) / "client"
            binary.write_bytes(struct.pack("<II", 0xFEEDFACF, 0x01000007))
            with self.assertRaises(ValueError):
                packager.validate_binary(binary, "arm64")

    def test_release_tag_must_match_the_source_version(self):
        version = packager.source_version(ROOT)
        self.assertEqual(packager.source_version(ROOT, f"v{version}"), version)
        with self.assertRaises(ValueError):
            packager.source_version(ROOT, "v999.999.999")


if __name__ == "__main__":
    unittest.main()
