//! How much processor the player burns while a track plays. A music player
//! should be nearly idle: the work is one decode pass per few hundred
//! milliseconds of audio and a handful of wake-ups per second.

#![cfg(target_os = "linux")]

mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::*;
use pm_engine::audio::{PlayerEvent, PlayerHandle, SimulatedSink, Sink};

/// utime + stime of this process, in seconds.
fn process_cpu_seconds() -> f64 {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap();
    let after_comm = &stat[stat.rfind(')').unwrap() + 2..];
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    let ticks: f64 = fields[11].parse::<f64>().unwrap() + fields[12].parse::<f64>().unwrap();
    ticks / 100.0 // _SC_CLK_TCK is 100 on Linux
}

/// Voluntary context switches of every thread: one per wake-up from sleep.
fn wakeups() -> u64 {
    let mut total = 0;
    for task in std::fs::read_dir("/proc/self/task").unwrap().flatten() {
        let status = std::fs::read_to_string(task.path().join("status")).unwrap_or_default();
        let name = status.lines().next().unwrap_or("");
        // Only the player's own threads; the test harness and the HTTP
        // server thread are not part of the cost being measured.
        if !(name.contains("pm-audio") || name.contains("pm-decode") || name.contains("pm-fetch")) {
            continue;
        }
        total += status
            .lines()
            .find_map(|l| l.strip_prefix("voluntary_ctxt_switches:"))
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(0);
    }
    total
}

fn thread_count() -> usize {
    std::fs::read_dir("/proc/self/task").unwrap().count()
}

#[test]
fn playing_a_track_in_real_time_is_nearly_free() {
    if !require_ffmpeg() { return; }
    let dir = tempfile::tempdir().unwrap();
    run_ffmpeg(
        dir.path(),
        &tone_args(&["-c:a", "aac", "-b:a", "96k", "-f", "hls", "-hls_time", "2", "-hls_list_size", "0", "index.m3u8"]),
    );
    let server = Server::start(dir.path().to_path_buf());
    let events = Arc::new(Mutex::new(Vec::new()));
    let log = events.clone();
    let mut slot = Some(SimulatedSink::new(44_100, 2, 1.0));
    let player = PlayerHandle::spawn(
        move || Ok(Box::new(slot.take().unwrap()) as Box<dyn Sink>),
        move |e| log.lock().unwrap().push(e),
    );
    player.play("1_1", &format!("{}/index.m3u8", server.base), 12.0, 0.0, lab_config());
    let wait = Instant::now();
    while !events.lock().unwrap().iter().any(|e| matches!(e, PlayerEvent::Playing { .. })) {
        assert!(wait.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(5));
    }
    // Let startup work (download, probe) settle, then measure steady playback.
    std::thread::sleep(Duration::from_secs(2));
    let cpu_before = process_cpu_seconds();
    let wake_before = wakeups();
    let wall = Instant::now();
    std::thread::sleep(Duration::from_secs(6));
    let used = process_cpu_seconds() - cpu_before;
    let seconds = wall.elapsed().as_secs_f64();
    let share = used / seconds;
    let wake_rate = (wakeups() - wake_before) as f64 / seconds;
    println!(
        "steady playback: {:.1} ms CPU per second of audio, {:.0} player wake-ups per second ({} threads)",
        share * 1000.0,
        wake_rate,
        thread_count()
    );
    assert!(wake_rate < 40.0, "the player thread wakes {wake_rate:.0} times per second");
    // Debug build, so generous: an optimised build is several times cheaper.
    assert!(share < 0.05, "playback uses {:.1}% of a core", share * 100.0);
    drop(player);
}

/// The network watcher only looks while audio plays or the window is focused:
/// each look is three UDP `connect`s (nothing is sent), and idle it never wakes.
#[test]
fn the_network_watcher_costs_nothing_idle_and_next_to_nothing_while_playing() {
    use pm_engine::net::{MonitorConfig, NetworkMonitor, SystemAddresses};

    fn watcher_wakeups() -> u64 {
        let mut total = 0;
        for task in std::fs::read_dir("/proc/self/task").unwrap().flatten() {
            let status = std::fs::read_to_string(task.path().join("status")).unwrap_or_default();
            if status.lines().next().unwrap_or("").contains("pm-net") {
                total += status
                    .lines()
                    .find_map(|l| l.strip_prefix("voluntary_ctxt_switches:"))
                    .and_then(|v| v.trim().parse::<u64>().ok())
                    .unwrap_or(0);
            }
        }
        total
    }

    /// utime + stime of the watcher thread alone (the other test in this
    /// binary plays audio at the same time).
    fn watcher_cpu_seconds() -> f64 {
        let mut ticks = 0.0;
        for task in std::fs::read_dir("/proc/self/task").unwrap().flatten() {
            let stat = std::fs::read_to_string(task.path().join("stat")).unwrap_or_default();
            let Some(end) = stat.rfind(')') else { continue };
            if !stat[..end].contains("(pm-net") {
                continue;
            }
            let fields: Vec<&str> = stat[end + 2..].split_whitespace().collect();
            ticks += fields[11].parse::<f64>().unwrap_or(0.0) + fields[12].parse::<f64>().unwrap_or(0.0);
        }
        ticks / 100.0
    }

    // One look per second: eight times the real rate (8 s), so the cost is visible.
    let config = MonitorConfig { poll: Duration::from_secs(1), confirm: Duration::from_millis(500) };
    let monitor = NetworkMonitor::spawn(Arc::new(SystemAddresses), config, |_| {});
    std::thread::sleep(Duration::from_millis(300));
    let idle_before = watcher_wakeups();
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(watcher_wakeups() - idle_before, 0, "nothing playing, window not focused: not a single wake-up");

    monitor.set_playing(true);
    std::thread::sleep(Duration::from_millis(300));
    let (cpu_before, wake_before, wall) = (watcher_cpu_seconds(), watcher_wakeups(), Instant::now());
    std::thread::sleep(Duration::from_secs(6));
    let seconds = wall.elapsed().as_secs_f64();
    let share = (watcher_cpu_seconds() - cpu_before) / seconds;
    let rate = (watcher_wakeups() - wake_before) as f64 / seconds;
    println!("network watcher at one look per second: {:.4} % of one core, {rate:.2} wake-ups/s (real rate is 8x lower)", share * 100.0);
    assert!(share < 0.005, "{:.3} %", share * 100.0);
    assert!(rate < 2.5, "{rate:.2} wake-ups/s");
    monitor.set_playing(false);
}
