#!/usr/bin/env python3
"""Build a universal macOS app, a drag-to-Applications DMG, and checksums."""
import argparse
import hashlib
import os
from pathlib import Path
import plistlib
import shutil
import shlex
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent


def run(*args, **kwargs):
    subprocess.run(args, cwd=ROOT, check=True, **kwargs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cargo", default="cargo")
    args = parser.parse_args()
    cargo = shutil.which(args.cargo) or args.cargo
    rustup = str(Path(cargo).with_name("rustup"))
    targets = ["aarch64-apple-darwin", "x86_64-apple-darwin"]
    run(rustup, "target", "add", *targets)
    env = dict(os.environ, MACOSX_DEPLOYMENT_TARGET="12.0")
    # Remove build-machine paths from release binaries and panic messages.
    flags = env.get("CARGO_ENCODED_RUSTFLAGS", "").split("\x1f") if env.get("CARGO_ENCODED_RUSTFLAGS") else shlex.split(env.get("RUSTFLAGS", ""))
    flags.extend([f"--remap-path-prefix={ROOT}=mac-link", f"--remap-path-prefix={Path.home()}=/build"])
    env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(flags)
    env.pop("RUSTFLAGS", None)
    for target in targets:
        run(cargo, "build", "--locked", "--release", "--target", target, env=env)

    manifest = plistlib.loads((ROOT / "packaging/Info.plist").read_bytes())
    version = manifest["CFBundleShortVersionString"]
    out = ROOT / "dist/release"
    out.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="mac-link-package-") as work:
        stage = Path(work) / "image"
        app = stage / "Mac Link.app"
        executable = app / "Contents/MacOS/mac-link"
        resources = app / "Contents/Resources"
        executable.parent.mkdir(parents=True)
        resources.mkdir(parents=True)
        shutil.copy2(ROOT / "packaging/Info.plist", app / "Contents/Info.plist")
        shutil.copy2(ROOT / "packaging/MacLink.icns", resources / "MacLink.icns")
        shutil.copy2(ROOT / "LICENSE", resources / "LICENSE")
        shutil.copy2(ROOT / "packaging/THIRD-PARTY-NOTICES.txt", resources / "THIRD-PARTY-NOTICES.txt")
        run("lipo", "-create", *(str(ROOT / "target" / target / "release/mac-link") for target in targets), "-output", str(executable))
        executable.chmod(0o755)
        arches = subprocess.check_output(["lipo", "-archs", str(executable)], text=True).split()
        if set(arches) != {"arm64", "x86_64"}:
            raise RuntimeError(f"Unexpected executable architectures: {arches}")
        # The app reads the system font on the destination Mac; it is not bundled.
        run("codesign", "--force", "--sign", "-", str(app))
        run("codesign", "--verify", "--strict", str(app))

        archive = out / f"Mac-Link-{version}-universal.zip"
        run("ditto", "-c", "-k", "--keepParent", str(app), str(archive))
        (stage / "Applications").symlink_to("/Applications", target_is_directory=True)
        shutil.copy2(ROOT / "LICENSE", stage / "LICENSE.txt")
        (stage / "Install Mac Link.txt").write_text(
            "Mac Link\n\n1. Drag Mac Link.app onto Applications.\n"
            "2. Eject this disk image and open Mac Link from Applications.\n"
            "3. On the host, choose Set up this Mac. On the client, choose Connect to a Mac.\n\n"
            "Requires macOS 12 or later; includes Apple silicon and Intel binaries.\n"
            "No Rust installation or administrator installer is required.\n"
            "Power changes separately request macOS administrator authorization.\n\n"
            "This community build is ad-hoc signed, not Apple notarized. If macOS blocks\n"
            "first launch, review System Settings > Privacy & Security > Open Anyway\n"
            "after attempting to open it. Do not disable Gatekeeper globally.\n\n"
            "Source and releases: https://github.com/Tekno-Imaji-Nyata/mac-link\n"
            "License: MIT; third-party notices are inside the app's Resources folder.\n",
            encoding="utf-8",
        )
        dmg = out / f"Mac-Link-{version}-universal.dmg"
        run("hdiutil", "create", "-volname", "Mac Link", "-srcfolder", str(stage), "-fs", "HFS+", "-format", "UDZO", "-ov", str(dmg))
        run("hdiutil", "verify", str(dmg))

    artifacts = [dmg, archive]
    checksums = "".join(f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n" for path in artifacts)
    (out / "SHA256SUMS.txt").write_text(checksums, encoding="utf-8")
    print(f"\nRelease assets: {out}")
    print(checksums)


if __name__ == "__main__":
    main()
