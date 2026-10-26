#!/usr/bin/env bash
# Collects what is needed to understand a failure, whatever happened: full logcat, a screenshot, the package
# and window state. Never fails. Usage: collect-artifacts.sh [out-dir]   (default $PM_OUT or ./out)
out="${1:-${PM_OUT:-$PWD/out}}"
mkdir -p "$out"
# Любая команда adb может зависнуть, если эмулятор мёртв: каждая ограничена по времени.
T() { timeout "${1}" "${@:2}"; }
pkg="${PM_PACKAGE:-com.dec.privatemusic2.android}"
{ date; ps aux | grep -E "qemu|emulator|adb" | grep -v grep | cut -c1-200; free -m; df -h / | tail -1; } > "$out/host-state.txt" 2>&1
T 20 adb devices -l > "$out/adb-devices.txt" 2>&1
# If adb lost the device but the emulator process still runs, restarting the server brings it back.
if ! grep -q "device " "$out/adb-devices.txt"; then T 20 adb kill-server; T 20 adb start-server; sleep 3; T 20 adb devices -l >> "$out/adb-devices.txt" 2>&1; fi
# Host evidence survives loss of the guest; never collect unrelated temporary files.
T 5 sudo -n dmesg --ctime 2>&1 | tail -200 > "$out/host-kernel-tail.txt"
cat /proc/sys/kernel/core_pattern > "$out/host-core-pattern.txt" 2>/dev/null || true
# Only emulator Crashpad minidumps from this disposable runner, never arbitrary /tmp files.
mkdir -p "$out/emulator-minidumps"
for crash_root in /tmp/android-"${USER:-runner}"/emu-crash-*.db; do
  [ -d "$crash_root" ] || continue
  find "$crash_root" -maxdepth 3 -type f -name '*.dmp' -size -64M -print0 2>/dev/null |
    xargs -0 -r cp -t "$out/emulator-minidumps" --
done
if ! T 5 adb get-state 2>/dev/null | grep -qx device; then
  echo "adb device unavailable; guest artifacts skipped" > "$out/guest-unavailable.txt"
  exit 0
fi
T 60 adb logcat -d -v threadtime > "$out/logcat-full.txt" 2>&1
T 30 adb exec-out screencap -p > "$out/screenshot-final.png" 2>/dev/null
T 30 adb shell dumpsys window 2>/dev/null | grep -E 'mCurrentFocus|mFocusedApp' > "$out/window-focus.txt"
T 30 adb shell dumpsys package "$pkg" > "$out/dumpsys-package.txt" 2>&1
T 30 adb shell dumpsys activity activities > "$out/dumpsys-activities.txt" 2>&1
T 30 adb shell ps -A 2>/dev/null | grep -E "$pkg|PID" > "$out/ps.txt"
T 30 adb shell dumpsys meminfo "$pkg" > "$out/meminfo.txt" 2>&1
T 30 adb shell dumpsys gfxinfo "$pkg" framestats > "$out/gfxinfo.txt" 2>&1
T 30 adb shell dumpsys media_session > "$out/media-session.txt" 2>&1
T 120 adb bugreport "$out/bugreport.zip" >/dev/null 2>&1 || rm -f "$out/bugreport.zip"
# Native crash tombstones are only readable with root on user builds; google_apis images allow it.
T 20 adb root >/dev/null 2>&1 && sleep 2 && T 60 adb pull /data/tombstones "$out/tombstones" >/dev/null 2>&1
echo "collected into $out:"; ls -l "$out" | head -50
exit 0
