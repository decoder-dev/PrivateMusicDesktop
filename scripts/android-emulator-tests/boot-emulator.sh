#!/usr/bin/env bash
# Creates a Pixel 6 AVD (API 34, Google APIs, x86_64, 2 GB RAM), boots it headless and waits until it is usable.
# One retry: boots on shared runners flake.
# Usage: scripts/android-emulator-tests/boot-emulator.sh      (SDK packages must be installed; ANDROID_HOME set)
# Environment: AVD_NAME (pm_ci), SYSTEM_IMAGE, PM_OUT (log directory), BOOT_TIMEOUT (seconds per attempt, 420)
set -euo pipefail

: "${ANDROID_HOME:?ANDROID_HOME is not set (source scripts/android-ci-env.sh)}"
avd="${AVD_NAME:-pm_ci}"
image="${SYSTEM_IMAGE:-system-images;android-34;google_apis;x86_64}"
out="${PM_OUT:-$PWD/out}"
boot_timeout="${BOOT_TIMEOUT:-420}"
mkdir -p "$out"
emulator_bin="$ANDROID_HOME/emulator/emulator"
[ -x "$emulator_bin" ] || { echo "::error::$emulator_bin is missing: install the 'emulator' SDK package" >&2; exit 1; }

echo "--- KVM"
if [ -e /dev/kvm ]; then ls -l /dev/kvm; else echo "::warning::/dev/kvm does not exist: the emulator will be unusably slow or refuse to start"; fi

echo "--- creating the AVD $avd from $image"
# Каталог AVD задаём явно: у раннера он может быть другим, а avdmanager про выбор молчит.
export ANDROID_AVD_HOME="${ANDROID_AVD_HOME:-$HOME/.android/avd}"
mkdir -p "$ANDROID_AVD_HOME"
echo "ANDROID_AVD_HOME=$ANDROID_AVD_HOME ANDROID_USER_HOME=${ANDROID_USER_HOME:-<unset>} ANDROID_SDK_HOME=${ANDROID_SDK_HOME:-<unset>} HOME=$HOME"
# `echo no` answers "create a custom hardware profile?".
echo no | avdmanager create avd --force --name "$avd" --package "$image" --device pixel_6
config="$ANDROID_AVD_HOME/$avd.avd/config.ini"
if [ ! -f "$config" ]; then
  echo "AVD config not at $config; looking for it" >&2
  avdmanager list avd >&2 || true
  found="$(find "$HOME" /root -name config.ini -path "*$avd.avd*" 2>/dev/null | head -1 || true)"
  [ -n "$found" ] && config="$found"
fi
[ -f "$config" ] || { echo "::error::AVD config $config not found after avdmanager create" >&2; exit 1; }
echo "AVD config: $config"
set_config() { # key value
  if grep -q "^$1=" "$config"; then sed -i "s|^$1=.*|$1=$2|" "$config"; else echo "$1=$2" >> "$config"; fi
}
set_config hw.ramSize 2048
set_config disk.dataPartition.size 6G
set_config hw.keyboard yes
set_config hw.gpu.enabled yes
set_config hw.gpu.mode swiftshader_indirect
echo "--- AVD config"; cat "$config"

emulator_pid=""
stop_emulator() {
  adb emu kill >/dev/null 2>&1 || true
  if [ -n "$emulator_pid" ] && kill -0 "$emulator_pid" 2>/dev/null; then
    kill "$emulator_pid" 2>/dev/null || true
    sleep 3
    kill -9 "$emulator_pid" 2>/dev/null || true
  fi
  adb kill-server >/dev/null 2>&1 || true
  emulator_pid=""
}

attempt() { # <number>
  local log="$out/emulator-boot-$1.log"
  echo "--- boot attempt $1 (log: $log)"
  adb start-server
  nohup "$emulator_bin" -avd "$avd" -no-window -no-audio -no-boot-anim -gpu swiftshader_indirect \
    -no-snapshot -wipe-data -memory 3072 -cores 4 -feature -Vulkan -accel on -netdelay none -netspeed full \
    > "$log" 2>&1 &
  emulator_pid=$!
  echo "emulator pid $emulator_pid"

  local waited=0
  # adb wait-for-device can block forever: poll instead.
  until adb devices | grep -qE 'emulator-[0-9]+\s+device'; do
    kill -0 "$emulator_pid" 2>/dev/null || { echo "emulator process exited before a device appeared" >&2; return 1; }
    [ "$waited" -lt "$boot_timeout" ] || { echo "no device after ${boot_timeout}s" >&2; return 1; }
    sleep 3; waited=$((waited + 3))
  done
  echo "device is visible after ${waited}s: $(adb devices | grep emulator)"
  until [ "$(timeout 15 adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = "1" ]; do
    kill -0 "$emulator_pid" 2>/dev/null || { echo "emulator process exited during boot" >&2; return 1; }
    [ "$waited" -lt "$boot_timeout" ] || { echo "sys.boot_completed not set after ${boot_timeout}s" >&2; return 1; }
    sleep 3; waited=$((waited + 3))
  done
  echo "sys.boot_completed after ${waited}s"
  # The package manager answers a bit after boot_completed.
  until timeout 15 adb shell pm path android 2>/dev/null | grep -q '^package:'; do
    [ "$waited" -lt "$boot_timeout" ] || { echo "package manager not ready after ${boot_timeout}s" >&2; return 1; }
    sleep 2; waited=$((waited + 2))
  done
  return 0
}

ok=0
for n in 1 2; do
  if attempt "$n"; then ok=1; break; fi
  echo "::warning::emulator boot attempt $n failed; last lines of its log:" >&2
  tail -n 40 "$out/emulator-boot-$n.log" >&2 || true
  stop_emulator
  sleep 5
done
[ "$ok" = 1 ] || { echo "::error::the emulator did not boot in two attempts (see emulator-boot-*.log artifacts; is KVM enabled?)" >&2; exit 1; }

echo "--- settings for stable tests"
adb shell settings put global window_animation_scale 0
adb shell settings put global transition_animation_scale 0
adb shell settings put global animator_duration_scale 0
adb shell settings put global stay_on_while_plugged_in 7
adb shell settings put system screen_off_timeout 1800000
adb shell settings put system accelerometer_rotation 0
adb shell settings put system user_rotation 0
adb shell input keyevent KEYCODE_WAKEUP || true
adb shell wm dismiss-keyguard || true
adb shell input keyevent 82 || true
adb logcat -G 16M || true
adb logcat -c || true
echo "emulator ready: API $(adb shell getprop ro.build.version.sdk | tr -d '\r'), $(adb shell getprop ro.product.model | tr -d '\r'), $(adb shell wm size | tr -d '\r')"
echo "$emulator_pid" > "$out/emulator.pid"
# A live copy of the log, written on the host: if the emulator dies mid-test, this is all that is left of it.
nohup adb logcat -v threadtime > "$out/logcat-live.txt" 2>&1 &
# Host-side watchdog: one line every 5 s about the emulator process and memory (a vanished emulator shows up here).
nohup bash -c 'while true; do echo "$(date +%T) emu=$(pgrep -f qemu-system -c) $(free -m | awk "NR==2{print \"mem_used_mb=\"\$3\" avail_mb=\"\$7}")"; sleep 5; done' > "$out/host-watch.txt" 2>&1 &
