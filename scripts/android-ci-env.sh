#!/usr/bin/env bash
# Environment for the Android CI jobs on a GitHub-hosted ubuntu runner. SOURCE it:
#   . scripts/android-ci-env.sh
# It finds the preinstalled SDK and JDK, drops the runner's own NDK variables (the build must use the
# exact NDK from scripts/android-env.sh, not whichever the image ships) and then sources
# scripts/android-env.sh. With GITHUB_ENV set it also exports the values to the following steps.

_ci_here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# --- Android SDK -------------------------------------------------------------------------------
if [ -z "${ANDROID_HOME:-}" ] || [ ! -d "${ANDROID_HOME:-}" ]; then
  for _d in "${ANDROID_SDK_ROOT:-}" /usr/local/lib/android/sdk "$HOME/Android/Sdk" "$HOME/android-sdk"; do
    if [ -n "$_d" ] && [ -d "$_d" ]; then ANDROID_HOME="$_d"; break; fi
  done
fi
if [ -z "${ANDROID_HOME:-}" ] || [ ! -d "${ANDROID_HOME:-}" ]; then
  echo "::error::No Android SDK found (looked at ANDROID_HOME, ANDROID_SDK_ROOT, /usr/local/lib/android/sdk). Set ANDROID_HOME." >&2
  return 1 2>/dev/null || exit 1
fi
export ANDROID_HOME ANDROID_SDK_ROOT="$ANDROID_HOME"

# --- JDK 17 (21 as a fallback) ---------------------------------------------------------------------
if [ -z "${JAVA_HOME_OVERRIDE:-}" ]; then
  for _j in "${JAVA_HOME_17_X64:-}" "${JAVA_HOME_21_X64:-}" /usr/lib/jvm/temurin-17-jdk-amd64 /usr/lib/jvm/temurin-21-jdk-amd64; do
    if [ -n "$_j" ] && [ -x "$_j/bin/java" ]; then JAVA_HOME="$_j"; break; fi
  done
else
  JAVA_HOME="$JAVA_HOME_OVERRIDE"
fi
if [ -z "${JAVA_HOME:-}" ] || [ ! -x "${JAVA_HOME:-}/bin/java" ]; then
  JAVA_HOME="$(dirname "$(dirname "$(readlink -f "$(command -v java)")")")"
fi
export JAVA_HOME
export PATH="$JAVA_HOME/bin:$PATH"

# --- NDK: the exact version, never the image's default -----------------------------------------------
unset ANDROID_NDK ANDROID_NDK_ROOT ANDROID_NDK_HOME ANDROID_NDK_LATEST_HOME NDK_HOME
# shellcheck source=android-env.sh
. "$_ci_here/android-env.sh"

# Gradle: no daemon between steps on a one-shot runner, a modest heap.
export GRADLE_OPTS="${GRADLE_OPTS:--Dorg.gradle.daemon=false}"

# cmdline-tools (sdkmanager, avdmanager, apkanalyzer) and the newest build-tools on PATH.
for _c in "$ANDROID_HOME/cmdline-tools/latest/bin" "$ANDROID_HOME/cmdline-tools/bin" "$ANDROID_HOME/tools/bin"; do
  [ -d "$_c" ] && PATH="$_c:$PATH"
done
export PATH="$ANDROID_HOME/emulator:$PATH"

echo "android-ci-env: ANDROID_HOME=$ANDROID_HOME"
echo "android-ci-env: JAVA_HOME=$JAVA_HOME ($("$JAVA_HOME/bin/java" -version 2>&1 | head -1))"
echo "android-ci-env: NDK_HOME=$NDK_HOME ($([ -d "$NDK_HOME" ] && echo present || echo MISSING - run scripts/android-ci-sdk.sh))"

if [ -n "${GITHUB_ENV:-}" ]; then
  {
    echo "ANDROID_HOME=$ANDROID_HOME"
    echo "ANDROID_SDK_ROOT=$ANDROID_SDK_ROOT"
    echo "JAVA_HOME=$JAVA_HOME"
    echo "NDK_HOME=$NDK_HOME"
    echo "ANDROID_NDK_HOME=$ANDROID_NDK_HOME"
    echo "CARGO_TARGET_DIR=$CARGO_TARGET_DIR"
    echo "CARGO_INCREMENTAL=$CARGO_INCREMENTAL"
    echo "CARGO_PROFILE_DEV_DEBUG=$CARGO_PROFILE_DEV_DEBUG"
    echo "CC_aarch64_linux_android=$CC_aarch64_linux_android"
    echo "CXX_aarch64_linux_android=$CXX_aarch64_linux_android"
    echo "AR_aarch64_linux_android=$AR_aarch64_linux_android"
    echo "CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER"
    echo "GRADLE_OPTS=$GRADLE_OPTS"
  } >> "$GITHUB_ENV"
  {
    echo "$JAVA_HOME/bin"
    echo "$NDK_BIN"
    echo "$ANDROID_HOME/platform-tools"
    echo "$ANDROID_HOME/emulator"
    for _c in "$ANDROID_HOME/cmdline-tools/latest/bin" "$ANDROID_HOME/cmdline-tools/bin"; do [ -d "$_c" ] && echo "$_c"; done
  } >> "${GITHUB_PATH:-/dev/null}"
fi
