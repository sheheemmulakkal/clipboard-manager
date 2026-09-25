//! One running instance per user (and profile), and log file rotation.
//!
//! The D-Bus name (GApplication) is the normal single-instance mechanism,
//! but it needs the session bus: a copy started without one (e.g. from the
//! package's post-install script) would run next to the real one. An
//! advisory file lock works regardless.

use std::fs::{File, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

/// `$XDG_STATE_HOME/clipboard-manager/instance.lock` (per profile).
pub fn lock_path() -> PathBuf {
    crate::paths::state_dir().join("instance.lock")
}

/// Become the running instance. The lock is held while the returned file
/// is open (i.e. until the process exits). `None` if another instance holds it.
pub fn try_lock(path: &Path) -> Option<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)
        .ok()?;
    file.try_lock().ok().map(|()| file)
}

/// Whether another process currently holds the lock.
pub fn is_locked(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    try_lock(path).is_none()
}

/// Wait up to `timeout` for the lock (a restarting instance may still be
/// shutting down).
pub fn lock_with_retry(path: &Path, timeout: std::time::Duration) -> Option<File> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some(f) = try_lock(path) {
            return Some(f);
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Keep the previous run's log as `<name>.1` instead of overwriting it.
pub fn rotate(path: &Path) {
    if path.exists() {
        let mut old = path.as_os_str().to_owned();
        old.push(".1");
        let _ = std::fs::rename(path, PathBuf::from(old));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("cm-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn second_lock_fails_while_first_is_held_and_works_after() {
        let p = tmpdir("lock").join("instance.lock");
        let first = try_lock(&p).expect("first lock");
        assert!(try_lock(&p).is_none(), "second instance must not get the lock");
        assert!(is_locked(&p));
        drop(first);
        assert!(!is_locked(&p));
        assert!(try_lock(&p).is_some());
    }

    #[test]
    fn rotate_keeps_previous_log() {
        let d = tmpdir("rotate");
        let log = d.join("app.log");
        std::fs::write(&log, "run 1").unwrap();
        rotate(&log);
        assert!(!log.exists());
        assert_eq!(std::fs::read_to_string(d.join("app.log.1")).unwrap(), "run 1");
        std::fs::write(&log, "run 2").unwrap();
        rotate(&log);
        assert_eq!(std::fs::read_to_string(d.join("app.log.1")).unwrap(), "run 2");
        rotate(&d.join("missing.log")); // no-op, no panic
    }
}
