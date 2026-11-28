fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        // The Oboe backend uses C++. Link its runtime into the JNI library so the
        // APK can load it without relying on another shared library.
        println!("cargo:rustc-link-arg-cdylib=-lc++_static");
        println!("cargo:rustc-link-arg-cdylib=-lc++abi");
        println!("cargo:rustc-link-arg-cdylib=-Wl,--no-undefined");
    }
}
