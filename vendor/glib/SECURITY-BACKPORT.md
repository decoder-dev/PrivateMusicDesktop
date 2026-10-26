# glib 0.18.5 compatibility backport

This directory contains the unmodified crates.io `glib` 0.18.5 source, apart from the two-line upstream fix in `src/variant_iter.rs`, this note and the test lockfile. The original license, authorship, API and package version are retained. Cargo cache marker files are not distributed.

- Original crate checksum: `233daaf6e83ae6a12a52055f568f9d7cf4671dabb78ff9560ab6da230ce00ee5`
- Advisory: [RUSTSEC-2024-0429 / GHSA-wrw7-89jp-8q8g](https://rustsec.org/advisories/RUSTSEC-2024-0429.html)
- Upstream fix: [gtk-rs/gtk-rs-core#1343](https://github.com/gtk-rs/gtk-rs-core/pull/1343), merged as `05dff0ee696f9bcd8617cd48c4b812d046d440cb`

`VariantStrIter::impl_get` now passes a mutable output pointer to GLib. The original optimized iterator tests reproduce a SIGSEGV without this fix. CI runs the same upstream tests in release mode.

Tauri’s GTK3/WebKit dependencies require the 0.18 API, so adding glib 0.20 beside it would leave the vulnerable 0.18 copy in the graph. The workspace patch replaces that copy with this backport instead. Remove this directory and the Cargo patch once the entire GTK/WebKit dependency chain supports an upstream version with the fix. Keep dependency monitoring enabled; the unchanged version number may still require a documented manual review of this advisory.

Verification: `cargo test --manifest-path vendor/glib/Cargo.toml --release --locked --lib variant_iter::tests`.
