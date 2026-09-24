//! Filesystem locations used by the app.
//!
//! Everything under the data and state directories can contain clipboard
//! contents (history, images, logs), so those directories are kept 0700.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// `CLIPBOARD_MANAGER_PROFILE=dev` runs a development build side by side
/// with an installed one: its own D-Bus name, data, config and state dirs.
fn profile() -> Option<String> {
    std::env::var("CLIPBOARD_MANAGER_PROFILE").ok()
}

fn app_dir_name(profile: Option<&str>) -> String {
    match profile.filter(|p| !p.is_empty()) {
        Some(p) => format!("clipboard-manager-{p}"),
        None => "clipboard-manager".into(),
    }
}

fn app_id(profile: Option<&str>) -> String {
    const BASE: &str = "io.github.sheheemmulakkal.ClipboardManager";
    match profile.filter(|p| !p.is_empty()) {
        Some(p) => {
            let mut c = p.chars();
            let cap: String = c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default();
            format!("{BASE}.{cap}")
        }
        None => BASE.into(),
    }
}

/// D-Bus application id for the current profile.
pub fn application_id() -> String {
    app_id(profile().as_deref())
}

fn app_dir() -> String {
    app_dir_name(profile().as_deref())
}

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
    let dir = dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join(app_dir());
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
        .join(app_dir())
        .join("config.toml")
}

/// `$XDG_STATE_HOME/clipboard-manager` (created, 0700) — log file, portal token.
pub fn state_dir() -> PathBuf {
    let dir = dirs::state_dir()
        .map(|d| d.join(app_dir()))
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

#[cfg(test)]
mod profile_tests {
    use super::*;

    #[test]
    fn dev_profile_uses_separate_names() {
        assert_eq!(app_dir_name(None), "clipboard-manager");
        assert_eq!(app_dir_name(Some("dev")), "clipboard-manager-dev");
        assert_eq!(app_id(None), "io.github.sheheemmulakkal.ClipboardManager");
        assert_eq!(app_id(Some("dev")), "io.github.sheheemmulakkal.ClipboardManager.Dev");
        assert_eq!(app_dir_name(Some("")), "clipboard-manager");
    }
}
