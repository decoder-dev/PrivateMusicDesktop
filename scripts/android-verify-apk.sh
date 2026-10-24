#!/usr/bin/env bash
# Checks that an APK contains what the Android app needs: the native library for the ABI, the
# manifest entries (package, permissions, PlaybackService as a mediaPlayback foreground service,
# LoginActivity) and, for release builds, no debuggable flag.
# Usage: scripts/android-verify-apk.sh <apk> <arm64-v8a|x86_64> <debug|release>
# Every failed check is reported before the script exits non-zero.
set -u   # no pipefail on purpose: `printf | grep -q` would fail with SIGPIPE on big inputs

apk="${1:?usage: $0 <apk> <abi> <debug|release>}"
abi="${2:?usage: $0 <apk> <abi> <debug|release>}"
profile="${3:?usage: $0 <apk> <abi> <debug|release>}"
[ -f "$apk" ] || { echo "::error::$apk does not exist" >&2; exit 1; }
: "${ANDROID_HOME:?ANDROID_HOME is not set}"

aapt2="$(find "$ANDROID_HOME/build-tools" -name aapt2 -type f 2>/dev/null | sort -V | tail -1)"
[ -n "$aapt2" ] || { echo "::error::aapt2 not found under $ANDROID_HOME/build-tools (is build-tools;35.0.0 installed?)" >&2; exit 1; }
echo "aapt2: $aapt2"
echo "apk: $apk ($(stat -c %s "$apk") bytes)"

failures=0
fail() { echo "::error::$*" >&2; failures=$((failures + 1)); }
ok() { echo "ok: $*"; }

listing="$(unzip -l "$apk")" || { echo "::error::$apk is not a readable zip" >&2; exit 1; }
badging="$("$aapt2" dump badging "$apk" 2>&1)" || { echo "$badging" >&2; fail "aapt2 dump badging failed"; badging=""; }
manifest="$("$aapt2" dump xmltree --file AndroidManifest.xml "$apk" 2>&1)" || { echo "$manifest" >&2; fail "aapt2 dump xmltree failed"; manifest=""; }

echo "--- badging (head)"; printf '%s\n' "$badging" | head -20
echo "--- native libraries"; printf '%s\n' "$listing" | grep -E ' lib/' || true

# Native library of the app (lib name from src-tauri/Cargo.toml [lib]).
if printf '%s\n' "$listing" | grep -q "lib/$abi/libprivate_music_desktop_lib\.so"; then
  ok "lib/$abi/libprivate_music_desktop_lib.so"
else
  fail "lib/$abi/libprivate_music_desktop_lib.so is missing from the APK"
fi
# The web UI is embedded in the .so by Tauri, but the plugin Kotlin code must be in the dex.
printf '%s\n' "$listing" | grep -q ' classes.*\.dex' && ok "classes.dex present" || fail "no classes.dex in the APK"

check_badging() { # <label> <fixed-string>
  if printf '%s\n' "$badging" | grep -qF -- "$2"; then ok "$1"; else fail "badging has no: $2"; fi
}
check_badging "package id" "package: name='com.dec.privatemusic2.android'"
for permission in REQUEST_INSTALL_PACKAGES INTERNET ACCESS_NETWORK_STATE FOREGROUND_SERVICE FOREGROUND_SERVICE_MEDIA_PLAYBACK POST_NOTIFICATIONS WAKE_LOCK; do
  check_badging "permission $permission" "uses-permission: name='android.permission.$permission'"
done
check_badging "launcher activity" "launchable-activity: name='com.dec.privatemusic2.android.MainActivity'"

# Manifest tree: services and activities by name; the service type as the numeric mediaPlayback flag (0x2).
check_manifest() { # <label> <regex>
  if printf '%s\n' "$manifest" | grep -Eq -- "$2"; then ok "$1"; else fail "manifest has no match for: $2"; fi
}
check_manifest "PlaybackService declared" 'android:name[^=]*="[^"]*PlaybackService"'
check_manifest "LoginActivity declared" 'android:name[^=]*="[^"]*LoginActivity"'
# The service element of PlaybackService, then its type: mediaPlayback is the flag 0x2 (aapt2 prints it as
# a number, or as the name on some versions).
service_block="$(printf '%s\n' "$manifest" | awk '
  function flush() { if (buf ~ /PlaybackService/) printf "%s", buf; buf = "" }
  /^ *E: / { match($0, /^ */); if (f && RLENGTH <= ind) { flush(); f = 0 } if ($0 ~ /^ *E: service/) { f = 1; ind = RLENGTH } }
  f { buf = buf $0 "\n" }
  END { flush() }')"
if printf '%s\n' "$service_block" | grep -Eq 'foregroundServiceType[^=]*=(\(type 0x[0-9a-f]+\))?((0x0*)?2( |$)|"?mediaPlayback)'; then
  ok "PlaybackService foregroundServiceType = mediaPlayback"
else
  fail "PlaybackService is not declared as a mediaPlayback foreground service. Its block was:"
  printf '%s\n' "$service_block" | head -20 >&2
fi

# apkanalyzer (cmdline-tools) is a second opinion on the manifest when available.
if command -v apkanalyzer >/dev/null 2>&1; then
  echo "--- apkanalyzer manifest print (services and activities)"
  apkanalyzer manifest print "$apk" 2>&1 | grep -E 'android:name=|foregroundServiceType' | head -30 || true
else
  echo "(apkanalyzer not installed: skipped the second opinion)"
fi

debuggable=false
if printf '%s\n' "$badging" | grep -q '^application-debuggable' \
  || printf '%s\n' "$manifest" | grep -Eq 'debuggable[^=]*=(true|\(type 0x12\)0xffffffff)'; then debuggable=true; fi
case "$profile" in
  debug)   [ "$debuggable" = true ] && ok "debuggable (debug build)" || fail "the debug APK is not debuggable: run-as and DevTools tests cannot work" ;;
  release) [ "$debuggable" = false ] && ok "not debuggable (release build)" || fail "the release APK is debuggable" ;;
esac

if [ "$failures" -gt 0 ]; then
  echo "::error::$failures check(s) failed for $apk" >&2
  echo "--- full manifest tree for diagnosis" >&2
  printf '%s\n' "$manifest" | head -200 >&2
  exit 1
fi
echo "all APK checks passed"
