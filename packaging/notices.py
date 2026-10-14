#!/usr/bin/env python3
"""Collect macOS dependency license texts from locked crates and pinned upstream commits."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import json
from pathlib import Path
import re
import subprocess
from urllib.error import HTTPError
from urllib.parse import urlparse
from urllib.request import urlopen

ROOT = Path(__file__).resolve().parent.parent


def license_file(path):
    name = path.name.lower()
    return path.is_file() and path.stat().st_size < 256_000 and (
        name.startswith(("license", "licence", "copying", "notice"))
        or name in ("ofl.txt", "ufl.txt")
    )


def collect(package):
    root = Path(package["manifest_path"]).parent
    files = sorted(path for path in root.rglob("*") if license_file(path))
    texts = [(str(path.relative_to(root)), path.read_text(encoding="utf-8", errors="replace")) for path in files]
    # Some workspace crates omit the root license from crates.io. Recover it
    # from the exact upstream commit recorded in the downloaded crate.
    if not texts:
        vcs_path = root / ".cargo_vcs_info.json"
        vcs = json.loads(vcs_path.read_text()) if vcs_path.exists() else {}
        commit = vcs.get("git", {}).get("sha1", "")
        repo = urlparse(package.get("repository") or "")
        if repo.hostname != "github.com" or not re.fullmatch(r"[a-f0-9]{40}", commit):
            raise RuntimeError(f"Cannot locate pinned license for {package['name']}")
        repo_path = "/".join(repo.path.strip("/").split("/")[:2]).removesuffix(".git")
        base = f"https://raw.githubusercontent.com/{repo_path}/{commit}/"
        for name in ("LICENSE-MIT", "LICENSE-APACHE", "LICENSE", "LICENSE.txt", "LICENSE.md", "LICENSE-ZLIB"):
            try:
                with urlopen(base + name, timeout=20) as response:
                    text = response.read(256_000).decode("utf-8")
                    texts.append((base + name, text))
            except HTTPError as error:
                if error.code != 404:
                    raise
        if not texts and package.get("license") == "MIT":
            # Older crates such as dispatch declare MIT in Cargo.toml but
            # ship no license file, including at their published source commit.
            # Preserve author attribution and include the declared license text.
            authors = ", ".join(package.get("authors", []))
            if not authors:
                raise RuntimeError(f"Missing MIT attribution for {package['name']}")
            mit = (ROOT / "LICENSE").read_text(encoding="utf-8")
            mit = re.sub(r"Copyright[^\n]*", f"Package authors: {authors}", mit, count=1)
            texts.append(("MIT declared in package manifest; no standalone upstream license file", mit))
        if not texts:
            raise RuntimeError(f"No upstream license found for {package['name']} {package['version']}")
    header = f"{package['name']} {package['version']}\nLicense expression: {package.get('license') or 'See license text'}\nSource: {package.get('repository') or 'crates.io'}\n"
    return header + "\n".join(f"\n--- {label} ---\n{text}" for label, text in texts)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cargo", default="cargo")
    args = parser.parse_args()
    packages = {}
    for target in ("aarch64-apple-darwin", "x86_64-apple-darwin"):
        result = subprocess.check_output([args.cargo, "metadata", "--locked", "--format-version", "1", "--filter-platform", target], cwd=ROOT)
        metadata = json.loads(result)
        resolved = {node["id"] for node in metadata["resolve"]["nodes"]}
        for package in metadata["packages"]:
            if package["source"] and package["id"] in resolved:
                packages[package["id"]] = package
    ordered = sorted(packages.values(), key=lambda p: (p["name"], p["version"]))
    with ThreadPoolExecutor(max_workers=8) as pool:
        sections = list(pool.map(collect, ordered))
    introduction = (
        "Mac Link — Third-party notices\n\n"
        "Generated from Cargo.lock for Apple silicon and Intel macOS. Includes build\n"
        "dependencies for completeness. Dependency code retains its own licenses;\n"
        "Mac Link's MIT license does not replace them. Alternative licenses are\n"
        "reported as declared by the upstream package. Bundled font licenses are\n"
        "included with epaint_default_fonts. Apple's system font is not redistributed.\n\n"
    )
    path = ROOT / "packaging/THIRD-PARTY-NOTICES.txt"
    content = introduction + ("\n\n" + "=" * 78 + "\n\n").join(sections)
    path.write_text("\n".join(line.rstrip() for line in content.splitlines()).rstrip() + "\n", encoding="utf-8")
    print(f"Collected notices for {len(sections)} packages: {path.name}")


if __name__ == "__main__":
    main()
