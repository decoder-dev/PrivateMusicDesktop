#!/usr/bin/env bash
# Replays the original Swift eviction loop (kept in main.swift) against the
# Rust policy on 20,000 random libraries; the chosen tracks must match.
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
