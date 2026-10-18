#!/usr/bin/env bash
# Сравнивает расчёт фильтров Rust с исходными формулами Swift из main.swift
# на сетке значений и 20 000 случайных входных наборов. Все коэффициенты
# должны совпадать побитно. Требуется swiftc.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$here/../../../.."
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
# Статическая библиотека построена без LTO, поэтому компоновщик Swift может ее прочитать.
CARGO_PROFILE_RELEASE_LTO=off RUSTFLAGS="-C embed-bitcode=no" CARGO_TARGET_DIR="$work/rust" \
  cargo build --release --manifest-path "$root/native/Cargo.toml"
swiftc -O -import-objc-header "$root/PrivateMusic/Native/PrivateMusicDSP.h" "$here/main.swift" \
  -Xlinker "$work/rust/release/libpm_native.a" -o "$work/eqcheck"
"$work/eqcheck"
