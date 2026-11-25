#!/usr/bin/env bash
# Sign one unsigned release APK with a temporary debug key so it can be installed.
# The key exists only for this run. It is not the stable release key: the APK will
# not update an install signed with that key, and a later run will not update this
# APK. The keystore is deleted before the script returns and is never written to
# the repository.
# Usage: scripts/android-debug-sign.sh <unsigned.apk> <signed.apk>
set -euo pipefail

unsigned="${1:?usage: $0 <unsigned.apk> <signed.apk>}"
signed="${2:?usage: $0 <unsigned.apk> <signed.apk>}"
[ -f "$unsigned" ] || { echo "::error::$unsigned does not exist" >&2; exit 1; }
: "${ANDROID_HOME:?ANDROID_HOME is not set}"

apksigner="$(find "$ANDROID_HOME/build-tools" -name apksigner -type f | sort -V | tail -1)"
[ -n "$apksigner" ] || { echo "::error::apksigner not found under $ANDROID_HOME/build-tools" >&2; exit 1; }
command -v keytool >/dev/null 2>&1 || { echo "::error::keytool not found" >&2; exit 1; }

dir="$(mktemp -d)"
trap 'rm -rf "$dir"' EXIT
ks="$dir/debug.jks"
# The debug password is the well-known Android debug password, not a release secret.
store_pass="android"
keytool -genkeypair -keystore "$ks" -storepass "$store_pass" -keypass "$store_pass" \
  -alias androiddebugkey -keyalg RSA -keysize 2048 -validity 10000 \
  -dname "CN=Android Debug,O=Android,C=US" >/dev/null
"$apksigner" sign --ks "$ks" --ks-key-alias androiddebugkey \
  --ks-pass "pass:${store_pass}" --key-pass "pass:${store_pass}" \
  --out "$signed" "$unsigned"
"$apksigner" verify "$signed" >/dev/null
echo "debug-signed: $signed"
