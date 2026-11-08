#!/usr/bin/env bash
# Replaces the runner's Android emulator with a pinned build.
#
# The runners moved from emulator 37.2.12 to 37.2.13 in November 2026; with 37.2.13 the host qemu process
# segfaulted (exit 139, "device offline") in about one emulator job in three, at random steps, while the
# same app and image ran clean on 37.2.12 for weeks. The archive is checked against its SHA-256; if it
# cannot be fetched, the preinstalled emulator stays and the job only warns.
# Revisit: try a newer build (repository2-3.xml lists them) and move the pin when it runs clean.
set -euo pipefail
: "${ANDROID_HOME:?ANDROID_HOME is not set (source scripts/android-ci-env.sh first)}"

build=16428233 # 37.2.12.0
sum=b08fc43d8608d2955f607f1b287beb525041e730086bdca3af152accac3af9c1
url="https://dl.google.com/android/repository/emulator-linux_x64-$build.zip"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
if ! curl -fsSL --retry 4 --retry-delay 5 --retry-all-errors --connect-timeout 20 --max-time 900 -o "$tmp/emulator.zip" "$url"; then
  echo "::warning::could not download emulator build $build; keeping the preinstalled emulator"
  exit 0
fi
if ! echo "$sum  $tmp/emulator.zip" | sha256sum -c --quiet; then
  echo "::warning::emulator build $build does not match its pinned checksum; keeping the preinstalled emulator"
  exit 0
fi
if ! unzip -q -o "$tmp/emulator.zip" -d "$tmp/unpacked" </dev/null; then
  echo "::warning::could not unpack emulator build $build; keeping the preinstalled emulator"
  exit 0
fi
[ -x "$tmp/unpacked/emulator/emulator" ] || { echo "::warning::unexpected archive layout; keeping the preinstalled emulator"; exit 0; }

run() { "$@"; }
[ -w "$ANDROID_HOME" ] || run() { sudo "$@"; }
# sdkmanager's bookkeeping file stays, so the SDK still lists the package as installed.
[ -f "$ANDROID_HOME/emulator/package.xml" ] && cp "$ANDROID_HOME/emulator/package.xml" "$tmp/unpacked/emulator/package.xml"
run rm -rf "$ANDROID_HOME/emulator"
run mv "$tmp/unpacked/emulator" "$ANDROID_HOME/emulator"
echo "emulator pinned to build $build: $(grep -m1 -o 'Revision=.*' "$ANDROID_HOME/emulator/source.properties" 2>/dev/null || echo '?')"
