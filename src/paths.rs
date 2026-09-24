//! Filesystem locations used by the app.
//!
//! Everything under the data and state directories can contain clipboard
//! contents (history, images, logs), so those directories are kept 0700.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const APP_DIR: &str = "clipboard-manager";

/// Create `dir` (and parents) if needed and restrict it to the owner.
pub fn ensure_private_dir(dir: &Path) {
    if let Err(e) = std::fs::create_dir_all(dir) {
        tracing::warn!("[paths] cannot create {}: {e}", dir.display());
        return;
    }
    if let Ok(meta) = std::fs::metadata(dir) {
        if meta.permissions().mode() & 0o777 != 0o700 {
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
    }
}

/// `$XDG_DATA_HOME/clipboard-manager` (created, 0700).
pub fn data_dir() -> PathBuf {
    let dir = dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join(APP_DIR);
    ensure_private_dir(&dir);
    dir
}

/// `data_dir()/images` (created, 0700).
pub fn image_dir() -> PathBuf {
    let dir = data_dir().join("images");
    ensure_private_dir(&dir);
    dir
}

pub fn history_file() -> PathBuf {
    data_dir().join("history.bin")
}

/// `$XDG_CONFIG_HOME/clipboard-manager/config.toml` (not created).
pub fn config_file() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR)
        .join("config.toml")
}

/// `$XDG_STATE_HOME/clipboard-manager` (created, 0700) — log file, portal token.
pub fn state_dir() -> PathBuf {
    let dir = dirs::state_dir()
        .map(|d| d.join(APP_DIR))
        .unwrap_or_else(data_dir);
    ensure_private_dir(&dir);
    dir
}

/// Lowercase hex of an image hash — the image's file name stem.
pub fn hex(hash: &[u8; 32]) -> String {
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn image_path(hash: &[u8; 32]) -> PathBuf {
    image_dir().join(format!("{}.png", hex(hash)))
}

pub fn thumb_path(hash: &[u8; 32]) -> PathBuf {
    image_dir().join(format!("{}_thumb.png", hex(hash)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn hex_encodes_lowercase() {
        let h = hex(&[0xab; 32]);
        assert_eq!(h.len(), 64);
        assert!(h.starts_with("abab"));
    }

    #[test]
    fn ensure_private_dir_creates_and_tightens() {
        let d = std::env::temp_dir().join(format!("cm-test-{}-paths/a/b", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        ensure_private_dir(&d);
        assert_eq!(std::fs::metadata(&d).unwrap().permissions().mode() & 0o777, 0o700);
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o775)).unwrap();
        ensure_private_dir(&d);
        assert_eq!(std::fs::metadata(&d).unwrap().permissions().mode() & 0o777, 0o700);
    }
}
