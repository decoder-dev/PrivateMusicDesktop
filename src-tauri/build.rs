fn main() {
    // Oboe (C++) is linked with libc++_static; rustc drives the link with plain clang, so the C++
    // ABI runtime (`__cxa_pure_virtual` and friends) has to be named explicitly on Android.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        println!("cargo:rustc-link-lib=static=c++abi");
    }
    tauri_build::build()
}
