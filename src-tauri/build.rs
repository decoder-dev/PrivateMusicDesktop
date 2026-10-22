use std::path::PathBuf;

fn main() {
    // Oboe (C++) is linked with libc++_static; rustc drives the link with plain clang, so the C++
    // ABI runtime (`__cxa_pure_virtual` and friends) has to be named explicitly on Android.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        let triple = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
            Ok("aarch64") => "aarch64-linux-android",
            Ok("x86_64") => "x86_64-linux-android",
            Ok("arm") => "arm-linux-androideabi",
            _ => "i686-linux-android",
        };
        let ndk = ["NDK_HOME", "ANDROID_NDK_HOME", "ANDROID_NDK_ROOT"].iter().find_map(|name| std::env::var_os(name));
        if let Some(ndk) = ndk {
            let prebuilt = PathBuf::from(ndk).join("toolchains/llvm/prebuilt");
            if let Ok(hosts) = std::fs::read_dir(prebuilt) {
                for host in hosts.flatten() {
                    let lib = host.path().join("sysroot/usr/lib").join(triple);
                    if lib.join("libc++abi.a").exists() {
                        println!("cargo:rustc-link-search=native={}", lib.display());
                    }
                }
            }
        }
        println!("cargo:rustc-link-lib=static=c++abi");
    }
    tauri_build::build()
}
