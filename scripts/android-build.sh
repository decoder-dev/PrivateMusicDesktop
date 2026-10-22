#!/usr/bin/env bash
# Builds one Android APK with the Tauri CLI and copies it to a predictable place.
# Usage: scripts/android-build.sh <release|debug> <aarch64|x86_64> <out-dir>
# Environment: ANDROID_VERSION_CODE_OVERRIDE (see app/build.gradle.kts) is passed through to Gradle,
#              TAURI_CLI (default: @tauri-apps/cli@2) selects the CLI package.
# Prints the path of the APK on the last line of stdout ("APK=<path>").
set -euo pipefail

profile="${1:?usage: $0 <release|debug> <aarch64|x86_64> <out-dir>}"
target="${2:?usage: $0 <release|debug> <aarch64|x86_64> <out-dir>}"
out="${3:?usage: $0 <release|debug> <aarch64|x86_64> <out-dir>}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Относительный путь считается от каталога, где скрипт запустили, а не от src-tauri.
case "$out" in /*) ;; *) out="$PWD/$out" ;; esac

# shellcheck source=android-ci-env.sh
. "$root/scripts/android-ci-env.sh"

version="$(grep -m1 '^version' "$root/Cargo.toml" | cut -d'"' -f2)"
case "$target" in
  aarch64) abi=arm64-v8a;  label=arm64 ;;
  x86_64)  abi=x86_64;     label=x86_64 ;;
  *) echo "::error::unknown target '$target' (expected aarch64 or x86_64)" >&2; exit 2 ;;
esac
case "$profile" in
  release) flags=() ;;
  debug)   flags=(--debug) ;;
  *) echo "::error::unknown profile '$profile' (expected release or debug)" >&2; exit 2 ;;
esac

apk_dir="$root/src-tauri/gen/android/app/build/outputs/apk"
# An APK left by an earlier build must never be mistaken for this one.
mkdir -p "$apk_dir"
find "$apk_dir" -type f -name '*.apk' -path "*/$profile/*" -delete

echo "::group::tauri android build ($profile, $target, version $version, versionCode override: ${ANDROID_VERSION_CODE_OVERRIDE:-none})"
cd "$root/src-tauri"
set -x
npx --yes "${TAURI_CLI:-@tauri-apps/cli@2}" android build "${flags[@]}" --target "$target" --apk
set +x
echo "::endgroup::"

# Normally universal/<profile>/app-universal-<profile>[-unsigned].apk; search instead of trusting the layout.
mapfile -t found < <(find "$apk_dir" -type f -name '*.apk' -path "*/$profile/*" -printf '%T@ %p\n' | sort -rn | cut -d' ' -f2-)
if [ "${#found[@]}" -eq 0 ]; then
  echo "::error::the build finished but no $profile APK exists under $apk_dir. Layout now:" >&2
  find "$apk_dir" -maxdepth 4 -print >&2 || true
  exit 1
fi
apk="${found[0]}"
echo "built: $apk (${#found[@]} candidate(s))"

mkdir -p "$out"
if [ "$profile" = release ]; then
  case "$apk" in
    *unsigned*) name="PrivateMusic_${version}_android-${label}-unsigned.apk" ;;
    *)          name="PrivateMusic_${version}_android-${label}.apk" ;;
  esac
else
  name="PrivateMusic_${version}_android-${label}-debug${ANDROID_VERSION_CODE_OVERRIDE:+-versioncode$ANDROID_VERSION_CODE_OVERRIDE}.apk"
fi
cp "$apk" "$out/$name"
ls -l "$out/$name"
echo "abi expected: $abi"
echo "APK=$out/$name"
