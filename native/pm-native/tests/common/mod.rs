#![allow(dead_code)]
use std::ffi::{c_char, c_int, c_void, CString};
use std::process::Command;

extern "C" {
    fn dlopen(path: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
}

pub struct Rng(pub u64);
impl Rng {
    pub fn u32(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 32) as u32
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.u32() as usize) % n.max(1)
    }
    pub fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 11) as f64 / (1u64 << 53) as f64) as f32 * 2.0 - 1.0
    }
}

pub fn build_reference(file: &str) -> Option<*mut c_void> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let c_file = root.join("tests/reference").join(file);
    let shim = root.join("tests/shim");
    let out = std::env::temp_dir().join(format!("pm_ref_{}_{}.so", file, std::process::id()));
    let status = Command::new("cc")
        .args(["-O2", "-ffp-contract=off", "-fPIC", "-shared", "-Wl,-Bsymbolic", "-I"])
        .arg(&shim)
        .arg("-I")
        .arg(root.join("../../PrivateMusic/Native"))
        .arg(&c_file)
        .arg("-lm")
        .arg("-o")
        .arg(&out)
        .status()
        .ok()?;
    if !status.success() {
        return None;
    }
    let path = CString::new(out.to_str()?).ok()?;
    let handle = unsafe { dlopen(path.as_ptr(), 2) };
    (!handle.is_null()).then_some(handle)
}

pub fn sym<T>(handle: *mut c_void, name: &str) -> T {
    let c = CString::new(name).unwrap();
    let p = unsafe { dlsym(handle, c.as_ptr()) };
    assert!(!p.is_null(), "missing {name}");
    unsafe { std::mem::transmute_copy(&p) }
}

pub fn same(a: &[f32], b: &[f32]) {
    assert_eq!(a.len(), b.len());
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        assert!(x.to_bits() == y.to_bits() || (x - y).abs() <= 1e-6, "index {i}: {x} vs {y}");
    }
}
