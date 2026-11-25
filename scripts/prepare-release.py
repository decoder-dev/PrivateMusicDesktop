#!/usr/bin/env python3
"""Prepare one complete release and the signed desktop updater manifest."""
import argparse
import datetime
import hashlib
import json
from pathlib import Path
import re
import shutil


def prepare(parts, android, out, version, repo):
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise ValueError("expected a stable semantic version")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repo):
        raise ValueError("invalid repository")
    # Installers are required. Updater signatures are required only when the
    # desktop signing key was used (any signature or updater archive is present).
    # A release with no signatures still publishes installable files.
    installers = {
        "*-setup.exe": f"PrivateMusic_{version}_windows-x64-setup.exe",
        "*.dmg": f"PrivateMusic_{version}_macos-universal.dmg",
        "*.deb": f"PrivateMusic_{version}_linux-amd64.deb",
        "*.AppImage": f"PrivateMusic_{version}_linux-amd64.AppImage",
    }
    signatures = {
        "*-setup.exe.sig": f"PrivateMusic_{version}_windows-x64-setup.exe.sig",
        "*.app.tar.gz": f"PrivateMusic_{version}_macos-universal.app.tar.gz",
        "*.app.tar.gz.sig": f"PrivateMusic_{version}_macos-universal.app.tar.gz.sig",
        "*.deb.sig": f"PrivateMusic_{version}_linux-amd64.deb.sig",
        "*.AppImage.sig": f"PrivateMusic_{version}_linux-amd64.AppImage.sig",
    }
    def take(pattern):
        found = [p for p in parts.rglob(pattern) if p.is_file() and p.stat().st_size]
        if len(found) > 1:
            raise ValueError(f"expected one {pattern}, found {len(found)}")
        return found[0] if found else None
    sources = {}
    for pattern, name in installers.items():
        found = take(pattern)
        if found is None:
            raise ValueError(f"expected one {pattern}, found 0")
        sources[name] = found
    signed = {name: take(pattern) for pattern, name in signatures.items()}
    updater = any(signed.values())
    if updater and any(path is None for path in signed.values()):
        raise ValueError("updater signatures are incomplete")
    if updater:
        sources.update({name: path for name, path in signed.items() if path is not None})
    apk_name = f"PrivateMusic_{version}_android-arm64.apk"
    apks = list(android.rglob("*.apk"))
    if len(apks) != 1 or apks[0].name != apk_name or not apks[0].stat().st_size:
        raise ValueError("expected one signed Android APK with the release version")
    sources[apk_name] = apks[0]
    out.mkdir(parents=True, exist_ok=False)
    for name, source in sources.items():
        shutil.copyfile(source, out / name)
    base = f"https://github.com/{repo}/releases/download/v{version}/"
    platforms = {}
    if updater:
        for target, suffix in {
            "windows-x86_64-nsis": "windows-x64-setup.exe",
            "darwin-aarch64-app": "macos-universal.app.tar.gz",
            "darwin-x86_64-app": "macos-universal.app.tar.gz",
            "linux-x86_64-appimage": "linux-amd64.AppImage",
            "linux-x86_64-deb": "linux-amd64.deb",
        }.items():
            name = f"PrivateMusic_{version}_{suffix}"
            signature = (out / (name + ".sig")).read_text().strip()
            if not signature:
                raise ValueError(f"missing signature for {name}")
            platforms[target] = {"url": base + name, "signature": signature}
    if updater:
        manifest = {"version": version, "notes": f"Private Music {version}",
                    "pub_date": datetime.datetime.now(datetime.timezone.utc).isoformat(), "platforms": platforms}
        (out / "latest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
    else:
        (out / "UPDATER_DISABLED").write_text(
            "Desktop updater signatures were not produced. Set TAURI_SIGNING_PRIVATE_KEY and TAURI_SIGNING_PRIVATE_KEY_PASSWORD.\n")
    lines = []
    for path in sorted(out.iterdir()):
        with path.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        lines.append(f"{digest}  {path.name}\n")
    (out / "SHA256SUMS.txt").write_text("".join(lines))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for option in ("parts", "android", "out"):
        parser.add_argument("--" + option, type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--repo", required=True)
    prepare(**vars(parser.parse_args()))
