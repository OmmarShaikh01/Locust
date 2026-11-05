//! Project identity, the one-swarm-per-project lock, and the recent-projects list.
//!
//! A project is a directory. Everything Locust keeps for it (agent sessions, usage, exports,
//! logs) lives under that directory, so two projects never share chats. A lock file stops a
//! second Locust from running a swarm in the same project at the same time.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Lock file inside `<project>/.locust/`.
const LOCK_NAME: &str = "swarm.lock";
/// Project identity file inside `<project>/.locust/`.
const IDENTITY_NAME: &str = "project.json";
/// A lock not refreshed for this long is treated as left behind by a crashed Locust.
pub const STALE_AFTER: Duration = Duration::from_secs(30);
/// How often the running Locust refreshes its lock.
pub const HEARTBEAT_EVERY: Duration = Duration::from_secs(5);
const MAX_RECENT: usize = 8;

/// Stable 64-bit FNV-1a hash over `parts`. Not secret and not for security; used for ids that
/// must be the same across runs and Rust versions (std's hasher gives no such promise).
pub fn fnv1a(parts: &[&str]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for b in part.as_bytes() {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        // Separator, so ("ab","c") and ("a","bc") differ.
        h ^= 0xff;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// An open project that this process holds the swarm lock for. Dropping it releases the lock.
#[derive(Debug)]
pub struct Project {
    pub root: PathBuf,
    lock_path: PathBuf,
}

/// Remove the `\\?\` prefix Windows adds when canonicalizing, so paths stay readable.
fn clean_path(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => p,
    }
}

impl Project {
    /// Open `path` as a project: it must be an existing directory. Takes the swarm lock and
    /// creates `.locust/`. Fails if another live Locust already holds the lock.
    pub fn open(path: &Path) -> Result<Project, String> {
        let root = fs::canonicalize(path)
            .map_err(|e| format!("cannot open project \"{}\": {e}", path.display()))?;
        let root = clean_path(root);
        if !root.is_dir() {
            return Err(format!("\"{}\" is not a directory", root.display()));
        }
        let dir = root.join(".locust");
        fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;

        let lock_path = dir.join(LOCK_NAME);
        acquire_lock(&lock_path, &root)?;
        let project = Project { root, lock_path };
        project.check_identity();
        Ok(project)
    }

    /// Short stable id for this project, from its canonical path.
    pub fn id(&self) -> String {
        format!("{:016x}", fnv1a(&[&self.root.to_string_lossy()]))
    }

    /// Refresh the lock so other Locust instances see it as live.
    pub fn heartbeat(&self) {
        if let Ok(f) = OpenOptions::new().write(true).open(&self.lock_path) {
            let _ = f.set_modified(SystemTime::now());
        }
    }

    /// If this directory was copied or moved from another project, its saved agent sessions
    /// belong to the old project. Drop them so chats never carry across projects.
    fn check_identity(&self) {
        let id_path = self.root.join(".locust").join(IDENTITY_NAME);
        let mine = self.root.to_string_lossy().to_string();
        let recorded = fs::read_to_string(&id_path)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v.get("root").and_then(|r| r.as_str().map(str::to_string)));
        if let Some(old) = &recorded
            && old != &mine
        {
            let _ = fs::remove_file(self.root.join(".state").join("acp_sessions.json"));
        }
        if recorded.as_deref() != Some(mine.as_str()) {
            let body = serde_json::json!({"root": mine, "id": self.id()});
            let _ = fs::write(&id_path, body.to_string());
        }
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        // Only remove a lock that is still ours.
        if lock_owner_pid(&self.lock_path) == Some(std::process::id()) {
            let _ = fs::remove_file(&self.lock_path);
        }
    }
}

fn lock_owner_pid(path: &Path) -> Option<u32> {
    fs::read_to_string(path)
        .ok()?
        .lines()
        .find_map(|l| l.strip_prefix("pid="))
        .and_then(|p| p.trim().parse().ok())
}

fn write_lock(path: &Path, root: &Path) -> std::io::Result<()> {
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    write!(f, "pid={}\nroot={}\n", std::process::id(), root.display())
}

fn acquire_lock(path: &Path, root: &Path) -> Result<(), String> {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut f) => write!(f, "pid={}\nroot={}\n", std::process::id(), root.display())
            .map_err(|e| format!("cannot write lock: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let owner = lock_owner_pid(path);
            let age = fs::metadata(path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| SystemTime::now().duration_since(t).ok())
                .unwrap_or(Duration::MAX);
            if owner == Some(std::process::id()) || age >= STALE_AFTER {
                write_lock(path, root).map_err(|e| format!("cannot take over lock: {e}"))
            } else {
                Err(format!(
                    "another Locust (pid {}) already runs a swarm in {}. Only one swarm per project.",
                    owner.map_or("unknown".to_string(), |p| p.to_string()),
                    root.display()
                ))
            }
        }
        Err(e) => Err(format!("cannot create lock {}: {e}", path.display())),
    }
}

/// Where the per-user recent-projects list lives. `LOCUST_HOME` overrides it (used by tests).
fn recent_path() -> Option<PathBuf> {
    let base = std::env::var_os("LOCUST_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(|h| PathBuf::from(h).join(".locust"))
        })?;
    Some(base.join("recent.json"))
}

/// Recently opened projects, newest first. Entries that no longer exist are dropped.
pub fn recent_projects() -> Vec<String> {
    let Some(path) = recent_path() else {
        return Vec::new();
    };
    let list: Vec<String> = fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    list.into_iter().filter(|p| Path::new(p).is_dir()).collect()
}

/// Remember `root` as the most recently opened project.
pub fn remember_project(root: &Path) {
    let Some(path) = recent_path() else { return };
    let me = root.to_string_lossy().to_string();
    let mut list: Vec<String> = recent_projects().into_iter().filter(|p| p != &me).collect();
    list.insert(0, me);
    list.truncate(MAX_RECENT);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(&list) {
        let _ = fs::write(path, json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "locust-proj-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn test_fnv_is_stable_and_separates_parts() {
        assert_eq!(fnv1a(&["a", "b"]), fnv1a(&["a", "b"]));
        assert_ne!(fnv1a(&["ab", "c"]), fnv1a(&["a", "bc"]));
        // Known FNV-1a vector guards against accidental algorithm changes.
        assert_eq!(fnv1a(&[]), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn test_open_rejects_missing_dir_and_files() {
        assert!(Project::open(Path::new("definitely/not/here/xyz")).is_err());
        let d = temp("file");
        let f = d.join("a.txt");
        fs::write(&f, "x").unwrap();
        assert!(Project::open(&f).is_err());
    }

    #[test]
    fn test_second_open_in_same_project_is_refused_while_lock_is_live() {
        let d = temp("lock");
        let first = Project::open(&d).unwrap();
        // Simulate another process by changing the recorded pid.
        let lock = d.join(".locust").join(LOCK_NAME);
        fs::write(&lock, "pid=999999\nroot=x\n").unwrap();
        let err = Project::open(&d).unwrap_err();
        assert!(err.contains("Only one swarm per project"), "{err}");
        drop(first);
    }

    #[test]
    fn test_stale_lock_is_taken_over() {
        let d = temp("stale");
        fs::create_dir_all(d.join(".locust")).unwrap();
        let lock = d.join(".locust").join(LOCK_NAME);
        fs::write(&lock, "pid=999999\nroot=x\n").unwrap();
        let old = SystemTime::now() - STALE_AFTER - Duration::from_secs(5);
        OpenOptions::new()
            .write(true)
            .open(&lock)
            .unwrap()
            .set_modified(old)
            .unwrap();
        let p = Project::open(&d).expect("stale lock must be taken over");
        assert_eq!(lock_owner_pid(&lock), Some(std::process::id()));
        drop(p);
    }

    #[test]
    fn test_drop_releases_lock() {
        let d = temp("drop");
        let p = Project::open(&d).unwrap();
        let lock = d.join(".locust").join(LOCK_NAME);
        assert!(lock.exists());
        drop(p);
        assert!(!lock.exists());
        // And it can be opened again.
        drop(Project::open(&d).unwrap());
    }

    #[test]
    fn test_moved_project_drops_saved_sessions() {
        let d = temp("moved");
        fs::create_dir_all(d.join(".state")).unwrap();
        fs::create_dir_all(d.join(".locust")).unwrap();
        fs::write(d.join(".state").join("acp_sessions.json"), "{}").unwrap();
        fs::write(
            d.join(".locust").join(IDENTITY_NAME),
            r#"{"root":"C:\\somewhere\\else"}"#,
        )
        .unwrap();
        let p = Project::open(&d).unwrap();
        assert!(!d.join(".state").join("acp_sessions.json").exists());
        drop(p);
        // Reopening in place keeps new sessions.
        fs::write(d.join(".state").join("acp_sessions.json"), "{}").unwrap();
        drop(Project::open(&d).unwrap());
        assert!(d.join(".state").join("acp_sessions.json").exists());
    }

    #[test]
    fn test_recent_projects_newest_first_and_capped() {
        let home = temp("home");
        // SAFETY: only this test touches LOCUST_HOME.
        unsafe { std::env::set_var("LOCUST_HOME", &home) };
        let a = temp("ra");
        let b = temp("rb");
        remember_project(&a);
        remember_project(&b);
        remember_project(&a);
        let list = recent_projects();
        assert_eq!(list[0], a.to_string_lossy());
        assert_eq!(list[1], b.to_string_lossy());
        assert_eq!(list.len(), 2);
        unsafe { std::env::remove_var("LOCUST_HOME") };
    }
}
