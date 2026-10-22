#!/usr/bin/env bash
# Runs the Kotlin unit tests of the plugin without an app or an emulator.
# Usage: scripts/android-test.sh [extra gradle args]   (default task: testDebugUnitTest)
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/android-sdk}"
export ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$ANDROID_HOME}"

# Tauri's Android library lives inside the `tauri` crate of the cargo registry.
# Without --offline first (a fresh CI runner has not downloaded the crates of every platform), then with.
manifest="$(cd "$here/../.." && { cargo metadata --format-version 1 --offline 2>/dev/null || cargo metadata --format-version 1; } \
  | python3 -c 'import json,sys; print(next(p["manifest_path"] for p in json.load(sys.stdin)["packages"] if p["name"]=="tauri"))')"
tauri_android="$(dirname "$manifest")/mobile/android"
[ -d "$tauri_android" ] || { echo "tauri android library not found at $tauri_android" >&2; exit 1; }

cd "$here/android"
# GRADLE=gradle uses an installed Gradle instead of downloading the wrapper's distribution.
if [ -n "${GRADLE:-}" ]; then gradle="$GRADLE"; else gradle=./gradlew; fi
exec "$gradle" --no-daemon -PtauriAndroidDir="$tauri_android" "${@:-testDebugUnitTest}"
