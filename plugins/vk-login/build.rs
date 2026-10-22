// The two commands are Rust-to-Kotlin only (see README): they are listed so the
// plugin has permission entries, but no capability should ever grant them.
const COMMANDS: &[&str] = &["start", "cancel"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).android_path("android").build();
}
