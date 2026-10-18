#!/usr/bin/env bash
# Сравнивает разбор HLS в Rust с исходной реализацией Swift.
# Oracle.swift содержит исходный код HLSSegmentExporter.swift, а main.swift
# выводит нормализованный результат разбора. Требуется swiftc на Linux или macOS.
# Запуск: native/pm-native/tools/hls_oracle/run.sh [count] [seed]
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
swiftc -O "$here/Oracle.swift" "$here/main.swift" -o "$work/oracle"
python3 "$here/gen_corpus.py" "$work/corpus" "${1:-4000}" "${2:-1}"
cargo build --release --manifest-path "$here/../../../Cargo.toml" --example hls_dump
"$work/oracle" "$work"/corpus/*.m3u8 > "$work/swift.txt"
"$here/../../../target/release/examples/hls_dump" "$work"/corpus/*.m3u8 > "$work/rust.txt"
# Пути различаются только временным каталогом; в остальном дампы сравниваются как есть.
diff "$work/swift.txt" "$work/rust.txt" && echo "identical: $(grep -c '^==' "$work/swift.txt") playlists"
