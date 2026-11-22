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


class MacPackageTests(unittest.TestCase):
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
