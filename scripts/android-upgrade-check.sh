#!/usr/bin/env bash
# Update check for Android: data written by the old build must survive installing a newer APK over it.
#
# Usage: scripts/android-upgrade-check.sh <old.apk> [<new.apk>]
#   Needs one device/emulator on adb (ANDROID_SERIAL to choose), a DEBUG build (run-as, DevTools), Node and
#   `npm install` done in scripts/android-emulator-tests.
#   Without <new.apk> the same file is reinstalled with `-r -d`. CI passes a build of the same sources with a
#   higher versionCode (ANDROID_VERSION_CODE_OVERRIDE), which is the realistic "update" install.
#
# What is written before the update and checked after it:
#   1. a plain file in the app's private files/ directory (written through run-as);
#   2. a localStorage value in the web UI (the settings live there);
#   3. a probe encrypted by the Android Keystore through SecureStorePlugin (files/secure/session.bin):
#      the file's SHA-256 must be unchanged and the probe must still decrypt, i.e. the Keystore key survived;
#   then the app must start and report the new versionCode, the same version name and a clean log.
#
# Limits: both APKs come from the same job and the same debug key, so this proves the install-over path and
# the data/Keystore survival, not a migration from an older RELEASE (no Android release exists yet) and not
# signature mismatch handling. The Keystore probe is not a VK session: the engine treats it as signed out.
set -euo pipefail

old="${1:?usage: $0 <old.apk> [<new.apk>]}"
new="${2:-$old}"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tests="$here/android-emulator-tests"
pkg="${PM_PACKAGE:-com.dec.privatemusic2.android}"
out="${PM_OUT:-$PWD/out}"
mkdir -p "$out"
export PM_OUT="$out" PM_PACKAGE="$pkg"
[ -f "$old" ] && [ -f "$new" ] || { echo "::error::APK not found: $old / $new" >&2; exit 2; }

say() { echo "==> $*"; }
fail() { echo "::error::upgrade check: $*" >&2; collect; exit 1; }
collect() {
  adb logcat -d -v threadtime > "$out/logcat-upgrade.txt" 2>&1 || true
  adb exec-out screencap -p > "$out/upgrade-final.png" 2>/dev/null || true
  grep -E "$pkg|AndroidRuntime|RustStdoutStderr|PrivateMusic" "$out/logcat-upgrade.txt" | tail -60 || true
}
node_call() { (cd "$tests" && node invoke.mjs "$@"); }
shell_app() { adb shell "run-as $pkg $*"; }

aapt2="$(find "${ANDROID_HOME:?ANDROID_HOME is not set}/build-tools" -name aapt2 -type f | sort -V | tail -1)"
[ -n "$aapt2" ] || fail "aapt2 not found under $ANDROID_HOME/build-tools"
# awk reads all of its input, so no upstream command dies of SIGPIPE under pipefail (sed|head -1 would).
installed_code() { adb shell dumpsys package "$pkg" | awk '!done && match($0, /versionCode=[0-9]+/) { print substr($0, RSTART + 12, RLENGTH - 12); done = 1 }'; }
code_of() { "$aapt2" dump badging "$1" | sed -n "s/.*versionCode='\([0-9]*\)'.*/\1/p" | head -1; }
name_of() { "$aapt2" dump badging "$1" | sed -n "s/.*versionName='\([^']*\)'.*/\1/p" | head -1; }
old_code="$(code_of "$old")"; new_code="$(code_of "$new")"
say "old: $old (versionCode $old_code, $(name_of "$old"))"
say "new: $new (versionCode $new_code, $(name_of "$new"))"
[ -n "$old_code" ] && [ -n "$new_code" ] || fail "could not read versionCode from the APKs"
install_flags=(-r -g)
if [ "$new_code" -le "$old_code" ]; then
  install_flags+=(-d)
  echo "::notice::the new APK has versionCode $new_code <= $old_code: reinstalling with -d (a weaker check than a real update)"
fi

marker="upgrade-marker-$RANDOM$RANDOM"

say "fresh install of the old build"
adb uninstall "$pkg" >/dev/null 2>&1 || true
adb install -r -g "$old"
adb shell logcat -c
installed="$(installed_code)"
[ "$installed" = "$old_code" ] || fail "after installing the old build versionCode is '$installed', expected $old_code"

say "starting the old build and writing markers"
adb shell am start -W -n "$pkg/.MainActivity" >/dev/null
node_call app_info >/dev/null || fail "the old build did not start / answer app_info"
shell_app "sh -c 'echo $marker > files/pm-upgrade-marker.txt'"
node_call --eval "localStorage.setItem('pm-upgrade-marker','$marker'), localStorage.getItem('pm-upgrade-marker')" | grep -q "$marker" \
  || fail "could not write the localStorage marker"
kept="$(node_call debug_secure_selftest '{"mode":"keep"}')" || fail "writing the Keystore probe failed"
say "Keystore probe: $kept"
[ "$kept" = '"ok"' ] || fail "the Keystore probe was not written: $kept"
hash_before="$(shell_app "sha256sum files/secure/session.bin" | cut -d' ' -f1)"
[ -n "$hash_before" ] || fail "files/secure/session.bin does not exist after the probe was written"
say "session.bin sha256 before: $hash_before"

say "installing the new build over the old one"
adb shell am force-stop "$pkg"
adb shell logcat -c
adb install "${install_flags[@]}" "$new"
installed="$(installed_code)"
[ "$installed" = "$new_code" ] || fail "after the update versionCode is '$installed', expected $new_code"

say "checking the data after the update"
[ "$(shell_app "cat files/pm-upgrade-marker.txt" | tr -d '\r\n')" = "$marker" ] || fail "files/pm-upgrade-marker.txt is gone or changed"
hash_after="$(shell_app "sha256sum files/secure/session.bin" | cut -d' ' -f1)"
[ "$hash_after" = "$hash_before" ] || fail "files/secure/session.bin changed ($hash_before -> $hash_after)"

adb shell am start -W -n "$pkg/.MainActivity" >/dev/null
info="$(node_call app_info)" || fail "the updated build does not start / answer app_info"
say "app_info after the update: $info"
expected_name="$(name_of "$new")"
case "$info" in *"\"version\":\"$expected_name\""*) ;; *) fail "app_info does not report version $expected_name: $info" ;; esac
node_call --eval "localStorage.getItem('pm-upgrade-marker')" | grep -q "$marker" || fail "the localStorage marker did not survive the update"
verified="$(node_call debug_secure_selftest '{"mode":"verify"}')" || fail "the Keystore probe no longer decrypts after the update (the key did not survive?)"
[ "$verified" = '"ok"' ] || fail "Keystore verify answered $verified"
node_call debug_secure_selftest '{"mode":"clean"}' >/dev/null || true
state="$(node_call session_state)" || fail "session_state failed after the update"
say "session_state after the update: $state"

crashes="$(adb logcat -d -v threadtime | grep -E "Process: $pkg|ANR in $pkg|>>> $pkg <<<" || true)"
[ -z "$crashes" ] || fail "crash evidence in the log after the update: $crashes"
shell_app "rm -f files/pm-upgrade-marker.txt" || true
collect >/dev/null
say "upgrade check passed: markers, Keystore probe and start-up survived $old_code -> $new_code"
