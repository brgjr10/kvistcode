#!/usr/bin/env python3
"""Writes the winget manifests for a published Kvistcode release.

  python3 scripts/winget_manifest.py v0.8.0            # downloads the installer, writes the files
  python3 scripts/winget_manifest.py v0.8.0 --sha256 <hex>   # skip the download

The files land in packaging/winget/manifests/p/pl0xuee/Kvistcode/<version>/, the same layout as
github.com/microsoft/winget-pkgs, so the folder can be copied into a pull request there (or
submitted with `wingetcreate submit <folder>`). winget installs the same per-user NSIS installer
as the website download, so the in-app updater keeps working for winget installs.
"""
import argparse
import hashlib
import re
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REPO = "pl0xuee/kvistcode"
PACKAGE_ID = "pl0xuee.Kvistcode"
SCHEMA = "1.6.0"


def installer_url(version: str) -> str:
    return f"https://github.com/{REPO}/releases/download/v{version}/Kvistcode_{version}_x64-setup.exe"


def sha256_of_url(url: str) -> str:
    req = urllib.request.Request(url, headers={"User-Agent": "kvistcode-winget-manifest"})
    h = hashlib.sha256()
    with urllib.request.urlopen(req, timeout=120) as r:
        while chunk := r.read(1 << 20):
            h.update(chunk)
    return h.hexdigest().upper()


def manifests(version: str, sha256: str) -> dict[str, str]:
    head = f"# yaml-language-server: $schema=https://aka.ms/winget-manifest.{{kind}}.{SCHEMA}.schema.json\n\n"
    common = f"PackageIdentifier: {PACKAGE_ID}\nPackageVersion: {version}\n"
    return {
        f"{PACKAGE_ID}.yaml": head.format(kind="version")
        + common
        + f"DefaultLocale: en-US\nManifestType: version\nManifestVersion: {SCHEMA}\n",
        f"{PACKAGE_ID}.installer.yaml": head.format(kind="installer")
        + common
        + "InstallerType: nullsoft\n"
        + "Scope: user\n"
        + "UpgradeBehavior: install\n"
        + "Installers:\n"
        + "- Architecture: x64\n"
        + f"  InstallerUrl: {installer_url(version)}\n"
        + f"  InstallerSha256: {sha256}\n"
        + f"ManifestType: installer\nManifestVersion: {SCHEMA}\n",
        f"{PACKAGE_ID}.locale.en-US.yaml": head.format(kind="defaultLocale")
        + common
        + "PackageLocale: en-US\n"
        + "Publisher: Kvistcode contributors\n"
        + f"PublisherUrl: https://github.com/{REPO}\n"
        + f"PublisherSupportUrl: https://github.com/{REPO}/issues\n"
        + "PackageName: Kvistcode\n"
        + f"PackageUrl: https://github.com/{REPO}\n"
        + "License: MIT\n"
        + f"LicenseUrl: https://github.com/{REPO}/blob/main/LICENSE\n"
        + "ShortDescription: Run Claude Code, Codex, Gemini, Grok and other coding agents side by side.\n"
        + "Tags:\n- ai\n- claude\n- codex\n- coding-agent\n- developer-tools\n"
        + f"ReleaseNotesUrl: https://github.com/{REPO}/releases/tag/v{version}\n"
        + f"ManifestType: defaultLocale\nManifestVersion: {SCHEMA}\n",
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("tag", help="release tag, e.g. v0.8.0")
    ap.add_argument("--sha256", help="installer SHA-256 (skips the download)")
    args = ap.parse_args()
    m = re.fullmatch(r"v?(\d+\.\d+\.\d+)", args.tag)
    if not m:
        print(f"not a version tag: {args.tag}", file=sys.stderr)
        return 1
    version = m.group(1)
    sha = args.sha256.upper() if args.sha256 else sha256_of_url(installer_url(version))
    if not re.fullmatch(r"[0-9A-F]{64}", sha):
        print("the SHA-256 must be 64 hex characters", file=sys.stderr)
        return 1
    out = ROOT / "packaging/winget/manifests/p/pl0xuee/Kvistcode" / version
    out.mkdir(parents=True, exist_ok=True)
    for name, text in manifests(version, sha).items():
        (out / name).write_text(text, encoding="utf-8")
    print(f"wrote {out.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
