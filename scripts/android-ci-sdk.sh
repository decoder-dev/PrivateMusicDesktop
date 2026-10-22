#!/usr/bin/env bash
# Installs the Android SDK packages the CI needs, if they are missing.
# Usage: scripts/android-ci-sdk.sh <sdkmanager package>...
#   e.g. scripts/android-ci-sdk.sh "ndk;27.2.12479018" "platforms;android-35"
# The SDK is preinstalled on GitHub runners at $ANDROID_HOME; this only fills the gaps.
set -euo pipefail

if [ "$#" -eq 0 ]; then echo "usage: $0 <package>..." >&2; exit 2; fi
: "${ANDROID_HOME:?ANDROID_HOME is not set (source scripts/android-ci-env.sh first)}"

find_sdkmanager() {
  command -v sdkmanager 2>/dev/null && return 0
  find "$ANDROID_HOME/cmdline-tools" -name sdkmanager -type f 2>/dev/null | sort | tail -1
}

sdkmanager_bin="$(find_sdkmanager || true)"
if [ -z "$sdkmanager_bin" ]; then
  # Last resort: the SDK came without command-line tools. Download them (version pinned).
  echo "::warning::sdkmanager not found under $ANDROID_HOME; downloading the command-line tools"
  tmp="$(mktemp -d)"
  curl -fsSL --retry 3 -o "$tmp/cmdline-tools.zip" \
    "https://dl.google.com/android/repository/commandlinetools-linux-11076708_latest.zip"
  unzip -q "$tmp/cmdline-tools.zip" -d "$tmp"
  mkdir -p "$ANDROID_HOME/cmdline-tools"
  rm -rf "$ANDROID_HOME/cmdline-tools/latest"
  mv "$tmp/cmdline-tools" "$ANDROID_HOME/cmdline-tools/latest"
  sdkmanager_bin="$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager"
fi
echo "sdkmanager: $sdkmanager_bin"

# If the SDK directory is not writable by this user (some images), use sudo and keep the environment.
run() { "$@"; }
if [ ! -w "$ANDROID_HOME" ]; then
  echo "::warning::$ANDROID_HOME is not writable; using sudo for sdkmanager"
  run() { sudo env "PATH=$PATH" "JAVA_HOME=${JAVA_HOME:-}" "$@"; }
fi

# Licences (errors are tolerated here: `yes` dies of SIGPIPE when sdkmanager exits, and a missing licence
# shows up as a clear error at the install below).
set +e
yes | run "$sdkmanager_bin" --sdk_root="$ANDROID_HOME" --licenses >/dev/null 2>&1
set -e

missing=()
for package in "$@"; do
  path="$ANDROID_HOME/${package//;//}"
  if [ -d "$path" ]; then
    echo "present: $package"
  else
    missing+=("$package")
  fi
done

if [ "${#missing[@]}" -gt 0 ]; then
  echo "installing: ${missing[*]}"
  set +e
  yes | run "$sdkmanager_bin" --sdk_root="$ANDROID_HOME" --install "${missing[@]}"
  status=${PIPESTATUS[1]}
  set -e
  if [ "$status" -ne 0 ]; then
    echo "::error::sdkmanager --install failed (exit $status) for: ${missing[*]}" >&2
    exit "$status"
  fi
fi

for package in "$@"; do
  path="$ANDROID_HOME/${package//;//}"
  [ -d "$path" ] || { echo "::error::$package is still missing ($path does not exist)" >&2; exit 1; }
done
echo "SDK packages ready: $*"
