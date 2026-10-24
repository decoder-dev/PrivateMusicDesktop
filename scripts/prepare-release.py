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
    # Require each artifact exactly once. No partial release or ambiguous payload.
    expected = {
        "*-setup.exe": f"PrivateMusic_{version}_windows-x64-setup.exe",
        "*-setup.exe.sig": f"PrivateMusic_{version}_windows-x64-setup.exe.sig",
        "*.dmg": f"PrivateMusic_{version}_macos-universal.dmg",
        "*.app.tar.gz": f"PrivateMusic_{version}_macos-universal.app.tar.gz",
        "*.app.tar.gz.sig": f"PrivateMusic_{version}_macos-universal.app.tar.gz.sig",
        "*.deb": f"PrivateMusic_{version}_linux-amd64.deb",
        "*.deb.sig": f"PrivateMusic_{version}_linux-amd64.deb.sig",
        "*.AppImage": f"PrivateMusic_{version}_linux-amd64.AppImage",
        "*.AppImage.sig": f"PrivateMusic_{version}_linux-amd64.AppImage.sig",
    }
    sources = {}
    for pattern, name in expected.items():
        found = [p for p in parts.rglob(pattern) if p.is_file() and p.stat().st_size]
        if len(found) != 1:
            raise ValueError(f"expected one {pattern}, found {len(found)}")
        sources[name] = found[0]
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
    manifest = {"version": version, "notes": f"Private Music {version}",
                "pub_date": datetime.datetime.now(datetime.timezone.utc).isoformat(), "platforms": platforms}
    (out / "latest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
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
