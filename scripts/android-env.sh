#!/usr/bin/env bash
# Environment for building the Android app. Source it:  . scripts/android-env.sh
# Every value can be overridden by setting it before sourcing.
: "${ANDROID_HOME:=/home/user/android-sdk}"
: "${ANDROID_NDK_VERSION:=27.2.12479018}"
export ANDROID_HOME ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$ANDROID_HOME}"
export NDK_HOME="${NDK_HOME:-$ANDROID_HOME/ndk/$ANDROID_NDK_VERSION}"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$NDK_HOME}"
# Keep build output out of the source tree (and off a small disk).
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/pm-android-target}"
export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
export CARGO_PROFILE_DEV_DEBUG="${CARGO_PROFILE_DEV_DEBUG:-0}"
# Plain `cargo build --target aarch64-linux-android` needs the NDK toolchain;
# the Tauri CLI sets these up itself.
NDK_BIN="$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"
export CC_aarch64_linux_android="${CC_aarch64_linux_android:-$NDK_BIN/aarch64-linux-android26-clang}"
export CXX_aarch64_linux_android="${CXX_aarch64_linux_android:-$NDK_BIN/aarch64-linux-android26-clang++}"
export AR_aarch64_linux_android="${AR_aarch64_linux_android:-$NDK_BIN/llvm-ar}"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="${CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER:-$CC_aarch64_linux_android}"
export PATH="$NDK_BIN:$ANDROID_HOME/platform-tools:$PATH"
