#!/usr/bin/env bash
# Compares the Rust filter design with the original Swift formulas (kept
# verbatim in main.swift) over a grid and 20,000 random inputs; every
# coefficient must match bit for bit. Needs swiftc.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$here/../../../.."
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
# The staticlib is built without LTO so the Swift linker can read it.
CARGO_PROFILE_RELEASE_LTO=off RUSTFLAGS="-C embed-bitcode=no" CARGO_TARGET_DIR="$work/rust" \
  cargo build --release --manifest-path "$root/native/Cargo.toml"
swiftc -O -import-objc-header "$root/PrivateMusic/Native/PrivateMusicDSP.h" "$here/main.swift" \
  -Xlinker "$work/rust/release/libpm_native.a" -o "$work/eqcheck"
"$work/eqcheck"
