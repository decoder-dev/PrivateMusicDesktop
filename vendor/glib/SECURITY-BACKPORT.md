# glib 0.18.5 compatibility backport

This directory contains the unmodified crates.io `glib` 0.18.5 source, apart from the two-line production fix in `src/variant_iter.rs`, five upstream-compatible test assertions in `src/collections/strv.rs`, this note and the test lockfile. The original license, authorship, API and package version are retained. Cargo cache marker files are not distributed.

- Original crate checksum: `233daaf6e83ae6a12a52055f568f9d7cf4671dabb78ff9560ab6da230ce00ee5`
- Advisory: [RUSTSEC-2024-0429 / GHSA-wrw7-89jp-8q8g](https://rustsec.org/advisories/RUSTSEC-2024-0429.html)
- Upstream fix: [gtk-rs/gtk-rs-core#1343](https://github.com/gtk-rs/gtk-rs-core/pull/1343), merged as `05dff0ee696f9bcd8617cd48c4b812d046d440cb`

`VariantStrIter::impl_get` now passes a mutable output pointer to GLib. The original optimized iterator tests reproduce a SIGSEGV without this fix. CI runs all upstream library tests in release mode.

Tauri’s GTK3/WebKit dependencies require the 0.18 API, so adding glib 0.20 beside it would leave the vulnerable 0.18 copy in the graph. The workspace patch replaces that copy with this backport instead. Remove this directory and the Cargo patch once the entire GTK/WebKit dependency chain supports an upstream version with the fix. Keep dependency monitoring enabled; the unchanged version number may still require a documented manual review of this advisory.

Verification: `cargo test --manifest-path vendor/glib/Cargo.toml --release --locked --lib`.

## Test compatibility

The 0.18.5 `IntoStrV` test read the null terminator using `slice.get_unchecked(4)` on a four-element slice. That violates the slice-index precondition even though the underlying allocation has a fifth terminator slot. Under the current optimizer it failed with contradictory-looking equal lengths. The five assertions now use raw-pointer access to the allocated terminator, matching [glib 0.20’s tests](https://github.com/gtk-rs/gtk-rs-core/blob/337dde55bb1665a0a1286e689ec2819383d4e299/glib/src/collections/strv.rs). Production StrV code is unchanged.

## Static-analysis review

- The reported null paths into ObjectRef, Value and the test Boxed type combine distinct `from_glib` generic implementations. A `char **` charset output converts to GString, not GObject/Value; the GError output is converted only in the non-null branch. Foreign functions populate their documented output arguments.
- `PtrSlice::truncate` drops the element and then clears its slot. `drop_in_place` runs the element destructor; it does not deallocate the containing slice buffer, so the following slot write remains valid.
- The reported logging source is the boolean result of UUID validation, not the UUID string. The trace incorrectly combines the boolean conversion with the separate LogLevel conversion.

These seven findings are reviewed individually as false positives, with the SARIF paths retained in the task’s review evidence. Scanning remains enabled for the vendored source.
