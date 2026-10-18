#!/usr/bin/env bash
# Сравнивает правила удаления кэша Rust с исходным циклом Swift из main.swift
# на 20 000 случайных библиотеках. Наборы выбранных треков должны совпадать.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$here/../../../.."
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
CARGO_PROFILE_RELEASE_LTO=off RUSTFLAGS="-C embed-bitcode=no" CARGO_TARGET_DIR="$work/rust" \
  cargo build --release --manifest-path "$root/native/Cargo.toml"
swiftc -O -import-objc-header "$root/PrivateMusic/Native/PrivateMusicOffline.h" "$here/main.swift" \
  -Xlinker "$work/rust/release/libpm_native.a" -o "$work/check"
"$work/check"
