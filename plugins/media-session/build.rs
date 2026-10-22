const COMMANDS: &[&str] = &["status", "open_battery_settings"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).android_path("android").build();
}
