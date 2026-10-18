//! Сохраняем журнал предыдущего запуска перед открытием нового.

use std::{
    fs::{File, OpenOptions},
    io,
    path::Path,
};

pub fn open_log(directory: &Path) -> io::Result<File> {
    std::fs::create_dir_all(directory)?;
    let current = directory.join("private-music.log");
    let previous = directory.join("private-music.previous.log");
    // Если резервная копия не записалась, дописываем текущий журнал, сохраняя следы сбоя.
    let append = current.exists() && std::fs::copy(&current, previous).is_err();
    OpenOptions::new().create(true).write(true).append(append).truncate(!append).open(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn failed_backup_keeps_the_current_log() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let directory = std::env::temp_dir().join(format!("pm-log-fallback-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(directory.join("private-music.previous.log")).unwrap();
        std::fs::write(directory.join("private-music.log"), b"old\n").unwrap();
        let mut file = open_log(&directory).unwrap();
        file.write_all(b"new\n").unwrap();
        drop(file);
        assert_eq!(std::fs::read(directory.join("private-music.log")).unwrap(), b"old\nnew\n");
        std::fs::remove_file(directory.join("private-music.log")).unwrap();
        std::fs::remove_dir(directory.join("private-music.previous.log")).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn restart_preserves_the_previous_run_and_starts_a_clean_log() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let directory = std::env::temp_dir().join(format!("pm-log-test-{}-{unique}", std::process::id()));
        let mut first = open_log(&directory).unwrap();
        first.write_all(b"first run\n").unwrap();
        drop(first);
        let mut second = open_log(&directory).unwrap();
        second.write_all(b"second run\n").unwrap();
        drop(second);
        assert_eq!(std::fs::read(directory.join("private-music.previous.log")).unwrap(), b"first run\n");
        assert_eq!(std::fs::read(directory.join("private-music.log")).unwrap(), b"second run\n");
        for name in ["private-music.log", "private-music.previous.log"] {
            std::fs::remove_file(directory.join(name)).unwrap();
        }
        std::fs::remove_dir(directory).unwrap();
    }
}
