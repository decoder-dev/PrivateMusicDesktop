use std::path::PathBuf;

fn main() {
    // Oboe (C++) is linked with libc++_static; rustc drives the link with plain clang, so the C++
    // ABI runtime (`__cxa_pure_virtual` and friends) has to be named explicitly on Android.
    // Only libc++abi.a is copied into OUT_DIR and searched: the NDK's per-triple sysroot directory
    // also holds libc.a, which would replace the system libc if the linker found it there.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        let triple = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
            Ok("aarch64") => "aarch64-linux-android",
            Ok("x86_64") => "x86_64-linux-android",
            Ok("arm") => "arm-linux-androideabi",
            _ => "i686-linux-android",
        };
        let ndk = ["NDK_HOME", "ANDROID_NDK_HOME", "ANDROID_NDK_ROOT"].iter().find_map(std::env::var_os);
        let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
        if let Some(ndk) = ndk {
            let prebuilt = PathBuf::from(ndk).join("toolchains/llvm/prebuilt");
            for host in std::fs::read_dir(prebuilt).into_iter().flatten().flatten() {
                let abi = host.path().join("sysroot/usr/lib").join(triple).join("libc++abi.a");
                if abi.exists() {
                    std::fs::copy(&abi, out.join("libc++abi.a")).expect("copy libc++abi.a");
                    println!("cargo:rustc-link-search=native={}", out.display());
                    break;
                }
            }
        }
        println!("cargo:rustc-link-lib=static=c++abi");
    }
    tauri_build::build()
}
