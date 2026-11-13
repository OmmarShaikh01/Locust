//! Small file logger and structured in-memory ring buffer.
//! Writes redacted lines to `.locust/locust.log`.
//! Rotates to `locust.log.1` at 1 MiB. Never panics on I/O errors.
//! Retains the latest 5,000 structured log entries in memory for IPC streaming.

use crate::redact::redact;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 1024 * 1024;
const LOG_DIR: &str = ".locust";
const LOG_NAME: &str = "locust.log";
const RING_BUFFER_CAPACITY: usize = 5000;

static NEXT_LOG_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Level {
    Debug = 0,
    Info = 1,
    Warn = 2,
    Error = 3,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogEntry {
    pub id: u64,
    pub timestamp: u64,
    pub level: Level,
    pub message: String,
}

struct Logger {
    dir: PathBuf,
    debug: bool,
    max_bytes: u64,
    buffer: VecDeque<LogEntry>,
}

impl Logger {
    fn write(&mut self, level: Level, msg: &str) -> LogEntry {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let redacted = redact(msg);
        let id = NEXT_LOG_ID.fetch_add(1, Ordering::Relaxed);
        let entry = LogEntry {
            id,
            timestamp: secs,
            level,
            message: redacted.clone(),
        };

        if self.buffer.len() >= RING_BUFFER_CAPACITY {
            self.buffer.pop_front();
        }
        self.buffer.push_back(entry.clone());

        if level != Level::Debug || self.debug {
            let line = format!("{secs} {} {}\n", level.as_str(), redacted);
            let _ = self.try_write(&line);
        }

        entry
    }

    fn try_write(&self, line: &str) -> std::io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let path = self.dir.join(LOG_NAME);
        let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if size > 0 && size + line.len() as u64 > self.max_bytes {
            let old = self.dir.join(format!("{LOG_NAME}.1"));
            let _ = fs::remove_file(&old);
            fs::rename(&path, &old)?;
        }
        let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
        f.write_all(line.as_bytes())
    }

    fn recent(&self, limit: usize, min_level: Option<Level>) -> Vec<LogEntry> {
        self.buffer
            .iter()
            .rev()
            .filter(|e| match min_level {
                Some(min) => e.level >= min,
                None => true,
            })
            .take(limit)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }
}

static LOGGER: Mutex<Option<Logger>> = Mutex::new(None);

/// Start the logger. `debug` enables `Level::Debug` lines on disk.
pub fn init(debug: bool) {
    let l = Logger {
        dir: PathBuf::from(LOG_DIR),
        debug,
        max_bytes: MAX_BYTES,
        buffer: VecDeque::with_capacity(RING_BUFFER_CAPACITY),
    };
    let _ = fs::create_dir_all(&l.dir);
    *LOGGER.lock().unwrap_or_else(|e| e.into_inner()) = Some(l);
}

/// Write one redacted line and add to ring buffer.
pub fn log(level: Level, msg: &str) {
    if let Some(l) = LOGGER.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        let _ = l.write(level, msg);
    }
}

/// Retrieve recent structured logs from memory.
pub fn recent_logs(limit: usize, min_level: Option<Level>) -> Vec<LogEntry> {
    if let Some(l) = LOGGER.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        l.recent(limit, min_level)
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("locust_log_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    fn logger(dir: &Path, debug: bool, max: u64) -> Logger {
        Logger {
            dir: dir.to_path_buf(),
            debug,
            max_bytes: max,
            buffer: VecDeque::new(),
        }
    }

    #[test]
    fn creates_dir_and_writes() {
        let d = tmp("create").join("nested");
        logger(&d, false, MAX_BYTES).write(Level::Info, "hello");
        let s = fs::read_to_string(d.join(LOG_NAME)).unwrap();
        assert!(s.contains(" INFO hello\n"));
        let _ = fs::remove_dir_all(d.parent().unwrap());
    }

    #[test]
    fn redacts_messages() {
        let d = tmp("redact");
        logger(&d, false, MAX_BYTES).write(Level::Warn, "x Bearer abcdefgh12345");
        let s = fs::read_to_string(d.join(LOG_NAME)).unwrap();
        assert!(s.contains("Bearer [REDACTED]") && !s.contains("abcdefgh12345"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn debug_filtered() {
        let d = tmp("debug");
        logger(&d, false, MAX_BYTES).write(Level::Debug, "hidden");
        assert!(!d.join(LOG_NAME).exists());
        logger(&d, true, MAX_BYTES).write(Level::Debug, "shown");
        assert!(
            fs::read_to_string(d.join(LOG_NAME))
                .unwrap()
                .contains("shown")
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn rotates() {
        let d = tmp("rotate");
        let mut l = logger(&d, false, 100);
        for i in 0..10 {
            l.write(Level::Info, &format!("message number {i} padding padding"));
        }
        assert!(d.join("locust.log.1").exists());
        assert!(fs::metadata(d.join(LOG_NAME)).unwrap().len() <= 100);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn ring_buffer_retains_and_filters() {
        let d = tmp("ring_buffer");
        let mut l = logger(&d, true, MAX_BYTES);
        l.write(Level::Debug, "debug message");
        l.write(Level::Info, "info message");
        l.write(Level::Error, "error message");

        let all = l.recent(10, None);
        assert_eq!(all.len(), 3);

        let errors = l.recent(10, Some(Level::Error));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "error message");

        let info_and_above = l.recent(10, Some(Level::Info));
        assert_eq!(info_and_above.len(), 2);
        let _ = fs::remove_dir_all(&d);
    }
}
