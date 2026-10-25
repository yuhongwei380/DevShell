#!/usr/bin/env python3
"""Build DevShell.app and ZIP/DMG packages on macOS using the system SDK."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import plistlib
import re
import shutil
import subprocess
import tempfile
from datetime import datetime

ROOT = Path(__file__).resolve().parent.parent.parent
# Release support covers the recent macOS releases (15, 26, 27); older system
# versions are out of scope. The value lands in MACOSX_DEPLOYMENT_TARGET,
# -mmacosx-version-min, Info.plist LSMinimumSystemVersion and build-info.json.
MINIMUM_MACOS = "15.0"


def run(*args, **kwargs):
    return subprocess.run([str(arg) for arg in args], check=True, **kwargs)


def write_update_manifests(output: Path, zip_path: Path, bundle: Path, archs, build_number: int, app_version: str) -> list:
    """Write one update descriptor per architecture for the built bundle ZIP.

    The built-in updater reads up-macos-<arch>.json and installs the same
    universal archive, so an Apple Silicon client never accepts the Intel
    descriptor and vice versa. Field names must match UpdateDescriptor in
    internal/app/updates.go.
    """
    notes_file = ROOT / "docs/release" / f"RELEASE-{app_version}.md"
    notes = notes_file.read_text(encoding="utf-8").strip() if notes_file.is_file() else f"{app_version} 更新：详情见 GitHub Releases。"
    if len(notes) > 16000:
        raise SystemExit(f"Release notes are {len(notes)} characters; the update descriptor accepts at most 16000.")
    zip_digest = hashlib.sha256(zip_path.read_bytes()).hexdigest()
    executable_digest = hashlib.sha256((bundle / "Contents/MacOS/DevShell").read_bytes()).hexdigest()
    manifests = []
    for machine in archs:
        manifest = output / f"up-macos-{machine}.json"
        manifest.write_text(json.dumps({
            "schemaVersion": 2, "build": build_number, "product": "DevShell",
            "platform": f"darwin-{machine}", "version": app_version, "notes": notes,
            "sha256": zip_digest, "size": zip_path.stat().st_size,
            "executableSHA256": executable_digest,
        }, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        manifests.append(manifest)
    return manifests


def build(args):
    if platform.system() != "Darwin":
        raise SystemExit("macOS + Xcode Command Line Tools are required. Run this script on a Mac or the macOS CI workflow.")
    for tool in ["go", "xcrun", "sips", "iconutil", "codesign", "ditto", "hdiutil"]:
        if not shutil.which(tool):
            raise SystemExit(f"Missing build tool: {tool}. Install Go and run xcode-select --install.")
    sdk = run("xcrun", "--sdk", "macosx", "--show-sdk-path", capture_output=True, text=True).stdout.strip()
    sdk_version = run("xcrun", "--sdk", "macosx", "--show-sdk-version", capture_output=True, text=True).stdout.strip()
    # A deployment target newer than the SDK yields a bundle whose load commands
    # disagree with the SDK used, so stop instead of shipping a mismatched pair.
    sdk_numbers = [int(part) for part in re.findall(r"\d+", sdk_version)[:2]] or [0, 0]
    if tuple(sdk_numbers + [0]) < tuple(int(part) for part in MINIMUM_MACOS.split(".")[:2]):
        raise SystemExit(f"The macOS SDK {sdk_version or 'unknown'} is older than the supported minimum {MINIMUM_MACOS}; build on a newer macOS (CI runner macos-15 or later).")
    jobs = min(args.jobs, os.cpu_count() or 1)
    run("go", "mod", "verify", cwd=ROOT)
    for name in ["xterm.js", "xterm.css", "addon-fit.js", "addon-serialize.js"]:
        if not (ROOT / "web/vendor" / name).is_file():
            raise SystemExit("Frontend dependencies are missing. Run npm ci && npm run vendor first.")
    source = (ROOT / "internal/app/updates.go").read_text()
    build_number = int(re.search(r"const ApplicationBuild uint64 = (\d+)", source)[1])
    package_version = re.search(r'const ApplicationPackageVersion = "([^"]+)"', source)[1]
    app_version = re.search(r'const ApplicationVersion = "([^"]+)"', source)[1]
    archs = ["arm64", "amd64"] if args.arch == "universal" else [args.arch]
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    bundle = output / "DevShell.app"
    if bundle.exists():
        raise SystemExit(f"Output already exists: {bundle}. Choose a new --output directory or move the old build.")
    with tempfile.TemporaryDirectory(prefix="devshell-macos-build-") as temporary:
        stage = Path(temporary)
        app = stage / "DevShell.app"
        macos = app / "Contents/MacOS"
        resources = app / "Contents/Resources"
        macos.mkdir(parents=True)
        resources.mkdir()
        binaries = []
        for arch in archs:
            binary = stage / f"DevShell-{arch}"
            env = dict(os.environ, GOOS="darwin", GOARCH=arch, CGO_ENABLED="1", SDKROOT=sdk,
                       MACOSX_DEPLOYMENT_TARGET=MINIMUM_MACOS, GOMAXPROCS=str(jobs))
            # A fixed minimum keeps the Intel and Apple Silicon slices consistent.
            for flag in ["CGO_CFLAGS", "CGO_LDFLAGS"]:
                env[flag] = f"{env.get(flag, '')} -mmacosx-version-min={MINIMUM_MACOS}".strip()
            run("go", "build", "-p", str(jobs), "-buildvcs=false", "-trimpath", "-tags", "desktop,production",
                "-ldflags", "-s -w", "-o", binary, ".", cwd=ROOT, env=env)
            binaries.append(binary)
        executable = macos / "DevShell"
        if len(binaries) > 1:
            run("xcrun", "lipo", "-create", *binaries, "-output", executable)
        else:
            shutil.copy2(binaries[0], executable)
        executable.chmod(0o755)
        run("xcrun", "lipo", executable, "-verify_arch", *["x86_64" if a == "amd64" else a for a in archs])

        iconset = stage / "DevShell.iconset"
        iconset.mkdir()
        for size in [16, 32, 128, 256, 512]:
            for scale in [1, 2]:
                suffix = "@2x" if scale == 2 else ""
                run("sips", "-z", size * scale, size * scale, ROOT / "build/devshell.png",
                    "--out", iconset / f"icon_{size}x{size}{suffix}.png", stdout=subprocess.DEVNULL)
        run("iconutil", "-c", "icns", iconset, "-o", resources / "DevShell.icns")
        info = {
            "CFBundleName": "DevShell", "CFBundleDisplayName": "DevShell",
            "CFBundleIdentifier": "cloud.bitsflow.devshell", "CFBundleExecutable": "DevShell",
            "CFBundlePackageType": "APPL", "CFBundleInfoDictionaryVersion": "6.0",
            "CFBundleShortVersionString": package_version,
            "CFBundleVersion": f"{(datetime.strptime(str(build_number)[:8], '%Y%m%d') - datetime(2020, 1, 1)).days}.{build_number % 1000 // 100}.{build_number % 100}",
            "DevShellBuild": str(build_number), "CFBundleIconFile": "DevShell.icns",
            "LSMinimumSystemVersion": MINIMUM_MACOS, "NSHighResolutionCapable": True,
            "NSPrincipalClass": "NSApplication", "LSApplicationCategoryType": "public.app-category.developer-tools",
            "NSHumanReadableCopyright": "Copyright © 2026 DevShell contributors. MIT License.",
            "NSLocalNetworkUsageDescription": "DevShell 使用 SSH、SFTP 和网络诊断连接你选择的局域网服务器。",
        }
        with (app / "Contents/Info.plist").open("wb") as stream:
            plistlib.dump(info, stream)
        (app / "Contents/PkgInfo").write_bytes(b"APPL????")
        shutil.copy2(ROOT / "LICENSE", resources / "LICENSE.txt")
        shutil.copy2(ROOT / "docs/MACOS.md", resources / "macOS-README.md")
        license_dirs = ["build/go-licenses", "build/font-licenses", "web/assets/fonts/licenses",
                        "web/assets/ui-fonts", "web/assets/group-emoji", "web/assets/system-logos", "web/vendor"]
        for folder in license_dirs:
            for path in (ROOT / folder).rglob("*"):
                if path.is_file() and (path.suffix == ".txt" or "license" in path.name.lower() or "ofl" in path.name.lower()):
                    target = resources / "licenses" / path.relative_to(ROOT)
                    target.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copy2(path, target)
        sign_args = ["--timestamp=none"] if args.sign_identity == "-" else ["--options", "runtime", "--timestamp"]
        run("codesign", "--force", "--sign", args.sign_identity, *sign_args, app)
        run("codesign", "--verify", "--deep", "--strict", "--verbose=2", app)
        shutil.copytree(app, bundle)
        zip_path = output / f"DevShell-macos-{args.arch}.zip"
        run("ditto", "-c", "-k", "--sequesterRsrc", "--keepParent", bundle, zip_path)
        disk = stage / "disk"
        disk.mkdir()
        shutil.copytree(app, disk / app.name)
        (disk / "Applications").symlink_to("/Applications")
        shutil.copy2(ROOT / "docs/MACOS.md", disk / "使用说明.md")
        dmg_path = output / f"DevShell-macos-{args.arch}.dmg"
        run("hdiutil", "create", "-volname", "DevShell", "-srcfolder", disk, "-format", "UDZO", dmg_path)
        run("hdiutil", "verify", dmg_path)
        manifests = write_update_manifests(output, zip_path, bundle, archs, build_number, app_version) if args.arch == "universal" else []
        files = [zip_path, dmg_path] + manifests
        checksums = {path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in files}
        (output / "SHA256SUMS.txt").write_text("".join(f"{digest}  {name}\n" for name, digest in checksums.items()))
        metadata = {"build": build_number, "version": app_version, "architectures": archs,
                    "minimumMacOS": MINIMUM_MACOS, "signature": "ad-hoc" if args.sign_identity == "-" else "Developer ID",
                    "notarized": False, "parallelJobs": jobs,
                    "go": run("go", "version", capture_output=True, text=True).stdout.strip(),
                    "sha256": checksums}
        (output / "build-info.json").write_text(json.dumps(metadata, indent=2) + "\n")
    print(f"Built: {bundle}\nZIP: {zip_path}\nDMG: {dmg_path}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arch", choices=["universal", "arm64", "amd64"], default="universal")
    parser.add_argument("--output", type=Path, default=ROOT / "build/macos")
    parser.add_argument("--sign-identity", default="-", help="Developer ID identity; default: local ad-hoc signature")
    parser.add_argument("--jobs", type=int, default=4, help="Maximum build parallelism (default: 4, capped by CPU count)")
    args = parser.parse_args()
    if args.jobs < 1:
        parser.error("--jobs must be at least 1")
    build(args)
