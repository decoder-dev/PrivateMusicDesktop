#!/usr/bin/env bash
# Collects what is needed to understand a failure, whatever happened: full logcat, a screenshot, the package
# and window state. Never fails. Usage: collect-artifacts.sh [out-dir]   (default $PM_OUT or ./out)
out="${1:-${PM_OUT:-$PWD/out}}"
mkdir -p "$out"
pkg="${PM_PACKAGE:-com.dec.privatemusic2.android}"
adb devices -l > "$out/adb-devices.txt" 2>&1
adb logcat -d -v threadtime > "$out/logcat-full.txt" 2>&1
adb exec-out screencap -p > "$out/screenshot-final.png" 2>/dev/null
adb shell dumpsys window 2>/dev/null | grep -E 'mCurrentFocus|mFocusedApp' > "$out/window-focus.txt"
adb shell dumpsys package "$pkg" > "$out/dumpsys-package.txt" 2>&1
adb shell dumpsys activity activities > "$out/dumpsys-activities.txt" 2>&1
adb shell ps -A 2>/dev/null | grep -E "$pkg|PID" > "$out/ps.txt"
adb shell dumpsys meminfo "$pkg" > "$out/meminfo.txt" 2>&1
adb bugreport "$out/bugreport.zip" >/dev/null 2>&1 || rm -f "$out/bugreport.zip"
# Native crash tombstones are only readable with root on user builds; google_apis images allow it.
adb root >/dev/null 2>&1 && sleep 2 && adb pull /data/tombstones "$out/tombstones" >/dev/null 2>&1
echo "collected into $out:"; ls -l "$out" | head -50
exit 0
