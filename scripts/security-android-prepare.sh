#!/usr/bin/env bash
# Prepare Kotlin analysis/dependency resolution without compiling the Rust app or downloading an NDK.
set -euo pipefail
. scripts/android-ci-env.sh
scripts/android-ci-sdk.sh "platforms;android-36" "build-tools;36.0.0"
metadata="$(mktemp)"
trap 'rm -f "$metadata"' EXIT
cargo metadata --locked --format-version 1 > "$metadata"
python3 - "$metadata" <<'PY'
import json
from pathlib import Path
import sys
packages=json.loads(Path(sys.argv[1]).read_text())["packages"]
tauri=next(p for p in packages if p["name"]=="tauri")
api=Path(tauri["manifest_path"]).parent/"mobile/android"
assert api.is_dir(), "Tauri Android sources missing"
for plugin in ["media-session", "vk-login"]:
    link=Path("plugins")/plugin/"android/.tauri/tauri-api"
    link.parent.mkdir(parents=True,exist_ok=True)
    if not link.exists():link.symlink_to(api, target_is_directory=True)
PY
