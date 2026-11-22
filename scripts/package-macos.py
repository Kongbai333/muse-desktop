#!/usr/bin/env python3
"""Package a native macOS executable; account data is never staged."""
import argparse
import hashlib
import pathlib
import plistlib
import re
import shutil
import struct
import subprocess
import sys
import tempfile


def source_version(repo, tag=""):
    match = re.search(r'^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?)"',
                      (repo / "Cargo.toml").read_text(encoding="utf-8"), re.MULTILINE)
    if not match:
        raise ValueError("Cannot determine source version")
    version = match.group(1)
    if tag and tag != f"v{version}":
        raise ValueError(f"Release tag {tag} does not match source version v{version}")
    return version


def validate_binary(binary, architecture):
    expected = {"arm64": 0x0100000C, "x64": 0x01000007}[architecture]
    with binary.open("rb") as stream:
        header = stream.read(8)
    if len(header) != 8 or struct.unpack("<II", header) != (0xFEEDFACF, expected):
        raise ValueError(f"Executable is not a native macOS {architecture} Mach-O binary")


def assemble_bundle(repo, binary, destination, architecture, version):
    validate_binary(binary, architecture)
    app = destination / "Muse.app"
    executable = app / "Contents/MacOS/Muse"
    resources = app / "Contents/Resources"
    executable.parent.mkdir(parents=True)
    resources.mkdir(parents=True)
    shutil.copy2(binary, executable)
    executable.chmod(0o755)
    metadata = {
        "CFBundleDevelopmentRegion": "zh_CN",
        "CFBundleDisplayName": "Muse",
        "CFBundleName": "Muse",
        "CFBundleExecutable": "Muse",
        "CFBundleIdentifier": "app.muse.desktop",
        "CFBundlePackageType": "APPL",
        "CFBundleShortVersionString": version,
        "CFBundleVersion": version,
        "CFBundleIconFile": "Muse.icns",
        "LSMinimumSystemVersion": "14.0",
        "NSHighResolutionCapable": True,
        "NSAppTransportSecurity": {"NSAllowsArbitraryLoadsInWebContent": True, "NSAllowsLocalNetworking": True},
    }
    (app / "Contents/Info.plist").write_bytes(plistlib.dumps(metadata))
    for name in ("LICENSE", "NOTICE", "THIRD_PARTY_NOTICES.md"):
        shutil.copy2(repo / name, resources / name)
    shutil.copytree(repo / "LICENSES", resources / "LICENSES")
    (resources / "README.txt").write_text(
        f"Muse Desktop {version} - macOS {architecture}\n\n"
        "Requires macOS 14 or later. Move Muse.app to Applications and open it.\n"
        "Data: ~/Library/Application Support/MuseDesktopNext\n"
        "Proxy credentials are protected by the login Keychain.\n"
        "This development build is ad-hoc signed; it is not Apple-notarized.\n"
        "Account data, cookies and proxy credentials are not included.\n",
        encoding="utf-8")
    return app


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=pathlib.Path, default=pathlib.Path("target/release/muse-desktop"))
    parser.add_argument("--output", type=pathlib.Path, default=pathlib.Path("dist"))
    parser.add_argument("--arch", choices=("arm64", "x64"), required=True)
    parser.add_argument("--tag", default="")
    args = parser.parse_args()
    if sys.platform != "darwin":
        raise SystemExit("macOS packaging must run on a real macOS build host")
    repo = pathlib.Path(__file__).resolve().parents[1]
    version = source_version(repo, args.tag)
    validate_binary(args.binary, args.arch)
    subprocess.run(["otool", "-hv", str(args.binary)], check=True, capture_output=True)
    args.output.mkdir(parents=True, exist_ok=True)
    output = args.output.resolve() / f"Muse-Desktop-{version}-macos-{args.arch}.app.zip"
    with tempfile.TemporaryDirectory(prefix="muse-macos-package-") as directory:
        staging = pathlib.Path(directory)
        app = assemble_bundle(repo, args.binary, staging, args.arch, version)
        iconset = staging / "Muse.iconset"
        iconset.mkdir()
        for size in (16, 32, 128, 256, 512):
            for scale in (1, 2):
                suffix = "@2x" if scale == 2 else ""
                target = iconset / f"icon_{size}x{size}{suffix}.png"
                subprocess.run(["sips", "-z", str(size * scale), str(size * scale),
                                str(repo / "assets/app/muse-256.png"), "--out", str(target)],
                               check=True, capture_output=True)
        subprocess.run(["iconutil", "--convert", "icns", str(iconset), "--output",
                        str(app / "Contents/Resources/Muse.icns")], check=True)
        subprocess.run(["codesign", "--force", "--deep", "--sign", "-", "--timestamp=none", str(app)], check=True)
        subprocess.run(["codesign", "--verify", "--deep", "--strict", str(app)], check=True)
        subprocess.run(["ditto", "-c", "-k", "--sequesterRsrc", "--keepParent", str(app), str(output)], check=True)
    digest = hashlib.sha256(output.read_bytes()).hexdigest()
    pathlib.Path(str(output) + ".sha256").write_text(f"{digest}  {output.name}\n", encoding="ascii")
    print(output)


if __name__ == "__main__":
    main()
