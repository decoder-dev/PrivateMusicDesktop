#!/usr/bin/env bash
# Compares the Rust HLS parser with the original Swift parsing code.
# Oracle.swift is that code, extracted verbatim from HLSSegmentExporter.swift
# before it was replaced; main.swift prints a normalized dump of its results.
# Needs a Swift toolchain (swiftc) on Linux or macOS:
#   native/pm-native/tools/hls_oracle/run.sh [count] [seed]
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
swiftc -O "$here/Oracle.swift" "$here/main.swift" -o "$work/oracle"
python3 "$here/gen_corpus.py" "$work/corpus" "${1:-4000}" "${2:-1}"
cargo build --release --manifest-path "$here/../../../Cargo.toml" --example hls_dump
"$work/oracle" "$work"/corpus/*.m3u8 > "$work/swift.txt"
"$here/../../../target/release/examples/hls_dump" "$work"/corpus/*.m3u8 > "$work/rust.txt"
# Paths differ only by the temp dir; the dumps are otherwise compared as is.
diff "$work/swift.txt" "$work/rust.txt" && echo "identical: $(grep -c '^==' "$work/swift.txt") playlists"
