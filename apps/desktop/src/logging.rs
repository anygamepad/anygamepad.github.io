//! The log: standard error and a file in the log directory, restarted at
//! each launch.
//!
//! Never log serial numbers, Bluetooth addresses or device paths
//! (AGENTS.md): the library's types redact paths in their `Debug` output,
//! and messages name devices by VID/PID and product name only.

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use log::{LevelFilter, Log, Metadata, Record};

struct Logger {
    start: Instant,
    file: Mutex<Option<File>>,
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let elapsed = self.start.elapsed();
        let line = format!(
            "{:>6}.{:03} {:<5} {}: {}\n",
            elapsed.as_secs(),
            elapsed.subsec_millis(),
            record.level(),
            record.target(),
            record.args()
        );
        let _ = std::io::stderr().write_all(line.as_bytes());
        if let Ok(mut file) = self.file.lock() {
            if let Some(file) = file.as_mut() {
                let _ = file.write_all(line.as_bytes());
            }
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.file.lock() {
            if let Some(file) = file.as_mut() {
                let _ = file.flush();
            }
        }
    }
}

/// Starts logging at `level`, to `file` as well when it can be created.
/// Only the first call has an effect.
pub fn init(level: LevelFilter, file: Option<&Path>) {
    let file = file.and_then(|path| {
        fs::create_dir_all(path.parent()?).ok()?;
        File::create(path).ok()
    });
    let logger = Logger {
        start: Instant::now(),
        file: Mutex::new(file),
    };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(level);
    }
}
