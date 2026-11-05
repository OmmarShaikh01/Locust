//! Small file logger. Writes redacted lines to `.locust/locust.log`.
//! Rotates to `locust.log.1` at 1 MiB. Never panics on I/O errors.

use crate::redact::redact;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 1024 * 1024;
const LOG_DIR: &str = ".locust";
const LOG_NAME: &str = "locust.log";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Error,
    Warn,
    Info,
    Debug,
}

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
        }
    }
}

struct Logger {
    dir: PathBuf,
    debug: bool,
    max_bytes: u64,
}

impl Logger {
    fn write(&self, level: Level, msg: &str) {
        if level == Level::Debug && !self.debug {
            return;
        }
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let line = format!("{secs} {} {}\n", level.as_str(), redact(msg));
        let _ = self.try_write(&line);
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
}

static LOGGER: Mutex<Option<Logger>> = Mutex::new(None);

/// Start the logger. `debug` enables `Level::Debug` lines.
pub fn init(debug: bool) {
    let l = Logger {
        dir: PathBuf::from(LOG_DIR),
        debug,
        max_bytes: MAX_BYTES,
    };
    let _ = fs::create_dir_all(&l.dir);
    *LOGGER.lock().unwrap_or_else(|e| e.into_inner()) = Some(l);
}

/// Write one redacted line. No-op before `init`.
pub fn log(level: Level, msg: &str) {
    if let Some(l) = LOGGER.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        l.write(level, msg);
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
        let l = logger(&d, false, 100);
        for i in 0..10 {
            l.write(Level::Info, &format!("message number {i} padding padding"));
        }
        assert!(d.join("locust.log.1").exists());
        assert!(fs::metadata(d.join(LOG_NAME)).unwrap().len() <= 100);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn io_error_no_panic() {
        let d = tmp("ioerr");
        fs::create_dir_all(&d).unwrap();
        let blocker = d.join("file");
        fs::write(&blocker, "x").unwrap();
        // dir path is under a file: create_dir_all fails, must not panic
        logger(&blocker.join("sub"), false, MAX_BYTES).write(Level::Error, "x");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn public_api_safe_before_init() {
        log(Level::Info, "no init");
    }
}
