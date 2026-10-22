#!/usr/bin/env bash
# Runs the emulator tests against a connected device or emulator (adb), the same ones as the CI "emulator" job.
#
#   scripts/android-emulator-tests/run-local.sh [--apk <debug.apk>] [--clear] [--upgrade] [--offline]
#
#   --apk      install this debug APK first (default: the debug APK built under src-tauri/gen/android, if any;
#              without one the app must already be installed)
#   --clear    `pm clear` the app first: the tests need the signed-out state, and this ERASES its data
#   --upgrade  also run scripts/android-upgrade-check.sh (UNINSTALLS the app and reinstalls it)
#   --offline  do not require internet (the VK sign-in page steps are skipped when the device is offline)
# Needs: adb with exactly one device (ANDROID_SERIAL to choose), Node 20+, a DEBUG build (DevTools and run-as).
# Output (screenshots, logcat, results.json, summary.md): scripts/android-emulator-tests/out (or PM_OUT).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
apk=""; clear=0; upgrade=0; network=1
while [ "$#" -gt 0 ]; do
  case "$1" in
    --apk) apk="${2:?--apk needs a path}"; shift 2 ;;
    --clear) clear=1; shift ;;
    --upgrade) upgrade=1; shift ;;
    --offline) network=0; shift ;;
    -h|--help) sed -n '2,13p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1 (try --help)" >&2; exit 2 ;;
  esac
done

command -v adb >/dev/null || { echo "adb is not on PATH (Android SDK platform-tools)" >&2; exit 1; }
command -v node >/dev/null || { echo "node is not on PATH" >&2; exit 1; }
adb start-server >/dev/null 2>&1 || true
count="$(adb devices | awk 'NR>1 && $2=="device"' | wc -l)"
if [ "$count" -ne 1 ] && [ -z "${ANDROID_SERIAL:-}" ]; then
  echo "expected exactly one device on adb, found $count. Attach one, or set ANDROID_SERIAL." >&2
  adb devices -l >&2
  exit 1
fi
if [ -z "$apk" ]; then
  apk="$(find "$root/src-tauri/gen/android/app/build/outputs/apk" -path '*/debug/*.apk' -printf '%T@ %p\n' 2>/dev/null | sort -rn | head -1 | cut -d' ' -f2- || true)"
fi

export PM_OUT="${PM_OUT:-$here/out}"
[ "$clear" = 1 ] && export PM_CLEAR=1
[ "$network" = 1 ] && export PM_REQUIRE_NETWORK=1
[ -n "$apk" ] && { echo "APK: $apk"; export PM_APK="$apk"; }

cd "$here"
[ -d node_modules/playwright ] || PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1 npm install --no-audit --no-fund
status=0
node run.mjs || status=$?
if [ "$upgrade" = 1 ]; then
  [ -n "$apk" ] || { echo "--upgrade needs an APK (--apk)" >&2; exit 2; }
  (cd "$root" && ANDROID_HOME="${ANDROID_HOME:?--upgrade needs ANDROID_HOME for aapt2}" "$root/scripts/android-upgrade-check.sh" "$apk") || status=$?
fi
echo "results in $PM_OUT"
exit "$status"
