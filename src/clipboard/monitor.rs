use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gdk4::prelude::*;

use crate::clipboard::entry::ClipboardEntry;
use crate::config::AppConfig;
use crate::platform::Platform;
use crate::store::Store;

const MAX_RAW_PIXELS: u64 = 3840 * 2160 * 4; // 4K cap (~33 MB raw)
const THUMB_W: i32 = 240;
const THUMB_H: i32 = 135;

/// Watches the system clipboard and records new text / images in the store.
///
/// Driven by GDK's clipboard `changed` signal (XFixes selection
/// notifications on X11), so nothing runs while the clipboard is idle.
pub struct ClipboardMonitor;

struct State {
    store:           Rc<RefCell<Box<dyn Store>>>,
    last_text:       RefCell<String>,
    last_image_hash: RefCell<Option<[u8; 32]>>,
    image_dir:       PathBuf,
    max_text_bytes:  usize,
    ignore_apps:     Vec<String>,
    platform:        Arc<dyn Platform>,
    paused:          Rc<Cell<bool>>,
    /// SHA-256 of texts skipped as secrets or ignored-app copies this
    /// session, so they are refused if they reappear later (e.g. when a
    /// clipboard-persistence daemon republishes them without the hint).
    refused:         RefCell<HashSet<[u8; 32]>>,
    started:         std::time::Instant,
    on_change:       Box<dyn Fn()>,
}

/// Changes reported this soon after start describe the pre-existing clipboard.
const STARTUP_QUIET: std::time::Duration = std::time::Duration::from_millis(1500);

fn digest(bytes: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    sha2::Sha256::digest(bytes).into()
}

/// Clipboard owners such as KeePassXC and KWallet add this MIME type to
/// mark the content as a password.
const PASSWORD_HINT_MIME: &str = "x-kde-passwordManagerHint";

fn is_secret_offer(mime_types: &[&str]) -> bool {
    mime_types.contains(&PASSWORD_HINT_MIME)
}

/// Whether the focused window (`classes` = its WM_CLASS instance and class)
/// is one of the user's ignored apps.
fn is_ignored_app(classes: &[String], ignored: &[String]) -> bool {
    classes.iter().any(|c| ignored.iter().any(|i| i.eq_ignore_ascii_case(c)))
}

#[derive(Debug, PartialEq, Eq)]
enum TextDecision {
    Capture,
    Ignore,
    TooLarge,
}

/// Decide what to do with clipboard text.
///
/// * `last` — the last text this monitor saw (the same change can be
///   delivered twice); only a duplicate while the store still has it.
/// * `in_store` — the history already contains this text.
/// * `refused` — this text was skipped earlier as a secret / ignored-app copy.
fn text_decision(text: &str, last: &str, max_bytes: usize, in_store: bool, refused: bool) -> TextDecision {
    if text.is_empty() || refused || (text == last && in_store) {
        TextDecision::Ignore
    } else if text.len() > max_bytes {
        TextDecision::TooLarge
    } else {
        TextDecision::Capture
    }
}

impl ClipboardMonitor {
    pub fn start(
        store:     Rc<RefCell<Box<dyn Store>>>,
        config:    &AppConfig,
        platform:  Arc<dyn Platform>,
        paused:    Rc<Cell<bool>>,
        on_change: impl Fn() + 'static,
    ) -> Self {
        let state = Rc::new(State {
            store,
            last_text:       RefCell::new(String::new()),
            last_image_hash: RefCell::new(None),
            image_dir:       crate::paths::image_dir(),
            max_text_bytes:  config.max_text_bytes,
            ignore_apps:     config.ignore_apps.clone(),
            platform,
            paused,
            refused:         RefCell::new(HashSet::new()),
            started:         std::time::Instant::now(),
            on_change:       Box::new(on_change),
        });

        // GDK clipboard is backend-agnostic: works on both X11 and Wayland.
        let clipboard = gdk4::Display::default()
            .expect("no GDK display")
            .clipboard();

        // Only changes from now on are recorded: whatever is on the clipboard
        // at startup may come from an app we would have ignored (and was
        // most likely recorded by the previous run already).
        let state_c = Rc::clone(&state);
        clipboard.connect_changed(move |cb| {
            crate::crash::guarded("clipboard change", || on_clipboard_changed(cb, &state_c));
        });

        Self
    }
}

fn on_clipboard_changed(clipboard: &gdk4::Clipboard, state: &Rc<State>) {
    // GDK announces the clipboard owner found at startup as a change. That
    // content predates this run (and may come from an app we'd ignore).
    if state.started.elapsed() < STARTUP_QUIET {
        tracing::debug!("[monitor] startup clipboard content — not recorded");
        return;
    }
    // Our own set_text / set_texture (the user picked an item in the popup).
    // The controller records that itself. Forget the last-seen content so
    // copying it again from another app still moves it to the top.
    if clipboard.is_local() {
        state.last_text.borrow_mut().clear();
        *state.last_image_hash.borrow_mut() = None;
        return;
    }
    // Capture paused by the user: whatever is copied now is never recorded.
    if state.paused.get() {
        tracing::debug!("[monitor] paused — change ignored");
        return;
    }

    let formats = clipboard.formats();
    let mime_types: Vec<glib::GString> = formats.mime_types().to_vec();
    let mime_refs: Vec<&str> = mime_types.iter().map(|m| m.as_str()).collect();
    let raw_targets = state.platform.clipboard_targets().unwrap_or_default();
    let raw_refs: Vec<&str> = raw_targets.iter().map(String::as_str).collect();
    if is_secret_offer(&mime_refs) || is_secret_offer(&raw_refs) {
        tracing::debug!("[monitor] password-manager content — not recorded");
        refuse_current_text(clipboard, state);
        return;
    }
    if !state.ignore_apps.is_empty() {
        if let Some(classes) = state.platform.active_window_class() {
            if is_ignored_app(&classes, &state.ignore_apps) {
                tracing::debug!("[monitor] copied in ignored app {classes:?} — not recorded");
                refuse_current_text(clipboard, state);
                return;
            }
        }
    }
    let has_image = formats.contain_mime_type("image/png")
        || formats.contain_mime_type("image/jpeg")
        || formats.contain_mime_type("image/gif");
    let has_text = formats.contains_type(glib::types::Type::STRING)
        || formats.contain_mime_type("text/plain")
        || formats.contain_mime_type("text/plain;charset=utf-8");

    let state = Rc::clone(state);
    if has_image && !has_text {
        clipboard.read_texture_async(
            None::<&gdk4::gio::Cancellable>,
            move |result: Result<Option<gdk4::Texture>, glib::Error>| {
                crate::crash::guarded("image capture", || match result {
                    Ok(Some(texture)) => capture_image(&state, &texture),
                    Ok(None) => tracing::debug!("[monitor] clipboard returned no texture"),
                    Err(e) => tracing::debug!("[monitor] texture read error: {e}"),
                });
            },
        );
    } else if has_text {
        clipboard.read_text_async(
            None::<&gdk4::gio::Cancellable>,
            move |result: Result<Option<glib::GString>, glib::Error>| {
                if let Ok(Some(text)) = result {
                    crate::crash::guarded("text capture", || capture_text(&state, text.to_string()));
                }
            },
        );
    }
}

/// Remember the current clipboard text as refused (never to be recorded).
fn refuse_current_text(clipboard: &gdk4::Clipboard, state: &Rc<State>) {
    let state = Rc::clone(state);
    clipboard.read_text_async(
        None::<&gdk4::gio::Cancellable>,
        move |result: Result<Option<glib::GString>, glib::Error>| {
            if let Ok(Some(text)) = result {
                crate::crash::guarded("refuse secret", || {
                    state.refused.borrow_mut().insert(digest(text.as_bytes()));
                });
            }
        },
    );
}

fn capture_text(state: &State, text: String) {
    let in_store = state.store.borrow().get_all().iter().any(|e| {
        matches!(&e.content, crate::clipboard::entry::ClipboardContent::Text(t) if *t == text)
    });
    let refused = state.refused.borrow().contains(&digest(text.as_bytes()));
    let decision = text_decision(&text, &state.last_text.borrow(), state.max_text_bytes, in_store, refused);
    match decision {
        TextDecision::Ignore => {}
        TextDecision::TooLarge => {
            tracing::info!(
                "[monitor] skipping {} byte text (max_text_bytes = {})",
                text.len(),
                state.max_text_bytes
            );
            *state.last_text.borrow_mut() = text;
        }
        TextDecision::Capture => {
            *state.last_image_hash.borrow_mut() = None;
            let preview: String = text.chars().take(60).collect();
            tracing::debug!("[monitor] captured: {:?}", preview);
            *state.last_text.borrow_mut() = text.clone();
            let id = state.store.borrow_mut().next_id();
            state.store.borrow_mut().add(ClipboardEntry::new_text(id, text));
            (state.on_change)();
        }
    }
}

fn capture_image(state: &State, texture: &gdk4::Texture) {
    let w = texture.width();
    let h = texture.height();
    let raw_size = (w as u64) * (h as u64) * 4;
    if raw_size > MAX_RAW_PIXELS {
        tracing::warn!("[monitor] image too large ({w}×{h}) — skipping");
        return;
    }

    // Save to a temp file, compute SHA-256 from file bytes,
    // then atomically rename to the final content-addressed path.
    let tmp_path = state.image_dir.join("_capture.tmp");
    if let Err(e) = texture.save_to_png(&tmp_path) {
        tracing::warn!("[monitor] failed to save texture: {e}");
        return;
    }

    let png_bytes = match std::fs::read(&tmp_path) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("[monitor] failed to read tmp png: {e}");
            let _ = std::fs::remove_file(&tmp_path);
            return;
        }
    };

    let hash = {
        use sha2::Digest;
        let digest = sha2::Sha256::digest(&png_bytes);
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&digest);
        arr
    };

    // Same change delivered twice → nothing to do (unless the image was
    // deleted from the history meanwhile: then it is recorded again).
    if *state.last_image_hash.borrow() == Some(hash) && state.store.borrow().contains_image_hash(&hash) {
        let _ = std::fs::remove_file(&tmp_path);
        return;
    }
    *state.last_image_hash.borrow_mut() = Some(hash);
    state.last_text.borrow_mut().clear();

    // Store already has this image: `add` below moves it to the top.
    if state.store.borrow().contains_image_hash(&hash) {
        let _ = std::fs::remove_file(&tmp_path);
        let id = state.store.borrow_mut().next_id();
        state.store.borrow_mut().add(ClipboardEntry::new_image(id, hash, w as u32, h as u32));
        tracing::debug!("[monitor] re-copied image {}", crate::paths::hex(&hash));
        (state.on_change)();
        return;
    }

    let full_path  = crate::paths::image_path(&hash);
    let thumb_path = crate::paths::thumb_path(&hash);

    // Rename tmp → final full image
    if let Err(e) = std::fs::rename(&tmp_path, &full_path) {
        tracing::warn!("[monitor] rename failed: {e}");
        let _ = std::fs::remove_file(&tmp_path);
        return;
    }

    // Generate thumbnail via gdk-pixbuf
    match gdk_pixbuf::Pixbuf::from_file_at_scale(&full_path, THUMB_W, THUMB_H, true) {
        Ok(pb) => {
            if let Err(e) = pb.savev(&thumb_path, "png", &[]) {
                tracing::warn!("[monitor] thumb save failed: {e}");
            }
        }
        Err(e) => tracing::warn!("[monitor] thumb scale failed: {e}"),
    }

    let id = state.store.borrow_mut().next_id();
    state.store.borrow_mut().add(ClipboardEntry::new_image(id, hash, w as u32, h as u32));
    tracing::debug!("[monitor] captured image {} ({w}×{h})", crate::paths::hex(&hash));
    (state.on_change)();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_manager_hint_marks_secret() {
        assert!(is_secret_offer(&["text/plain", "x-kde-passwordManagerHint"]));
        assert!(!is_secret_offer(&["text/plain", "UTF8_STRING"]));
    }

    #[test]
    fn ignored_apps_match_instance_or_class_case_insensitively() {
        let list = vec!["keepassxc".to_string(), "1Password".to_string()];
        let win = |a: &str, b: &str| vec![a.to_string(), b.to_string()];
        assert!(is_ignored_app(&win("keepassxc", "KeePassXC"), &list));
        assert!(is_ignored_app(&win("1password", "1Password"), &list));
        assert!(!is_ignored_app(&win("firefox", "Firefox"), &list));
        assert!(!is_ignored_app(&[], &list));
        assert!(!is_ignored_app(&win("keepassxc", "KeePassXC"), &[]));
    }

    #[test]
    fn text_capture_rules() {
        let d = |text, last, stored, refused| text_decision(text, last, 10, stored, refused);
        assert_eq!(d("", "", false, false), TextDecision::Ignore);
        assert_eq!(d("same", "same", true, false), TextDecision::Ignore);
        assert_eq!(d("new", "old", false, false), TextDecision::Capture);
        assert_eq!(d("12345678901", "old", false, false), TextDecision::TooLarge);
        assert_eq!(d("1234567890", "old", false, false), TextDecision::Capture);
    }

    #[test]
    fn recopy_after_delete_is_captured() {
        // "X" was captured, then deleted from the history; copying it again
        // must record it even though it is still the last text seen.
        assert_eq!(text_decision("X", "X", 10, false, false), TextDecision::Capture);
    }

    #[test]
    fn refused_secret_is_never_captured() {
        // Content skipped earlier (password hint / ignored app) that shows up
        // again later, e.g. republished by a clipboard-persistence daemon.
        assert_eq!(text_decision("hunter2", "", 10, false, true), TextDecision::Ignore);
    }
}
