use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gdk4::prelude::*;

use crate::clipboard::entry::ClipboardEntry;
use crate::config::AppConfig;
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
    on_change:       Box<dyn Fn()>,
}

#[derive(Debug, PartialEq, Eq)]
enum TextDecision {
    Capture,
    Ignore,
    TooLarge,
}

/// Decide what to do with clipboard text, given the last text we saw.
fn text_decision(text: &str, last: &str, max_bytes: usize) -> TextDecision {
    if text.is_empty() || text == last {
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
        on_change: impl Fn() + 'static,
    ) -> Self {
        let state = Rc::new(State {
            store,
            last_text:       RefCell::new(String::new()),
            last_image_hash: RefCell::new(None),
            image_dir:       crate::paths::image_dir(),
            max_text_bytes:  config.max_text_bytes,
            on_change:       Box::new(on_change),
        });

        // GDK clipboard is backend-agnostic: works on both X11 and Wayland.
        let clipboard = gdk4::Display::default()
            .expect("no GDK display")
            .clipboard();

        {
            let state = Rc::clone(&state);
            clipboard.connect_changed(move |cb| on_clipboard_changed(cb, &state));
        }
        // Pick up whatever is on the clipboard at startup.
        on_clipboard_changed(&clipboard, &state);

        Self
    }
}

fn on_clipboard_changed(clipboard: &gdk4::Clipboard, state: &Rc<State>) {
    // Our own set_text / set_texture (the user picked an item in the popup).
    // The controller records that itself.
    if clipboard.is_local() {
        return;
    }

    let formats = clipboard.formats();
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
            move |result: Result<Option<gdk4::Texture>, glib::Error>| match result {
                Ok(Some(texture)) => capture_image(&state, &texture),
                Ok(None) => tracing::debug!("[monitor] clipboard returned no texture"),
                Err(e) => tracing::debug!("[monitor] texture read error: {e}"),
            },
        );
    } else if has_text {
        clipboard.read_text_async(
            None::<&gdk4::gio::Cancellable>,
            move |result: Result<Option<glib::GString>, glib::Error>| {
                if let Ok(Some(text)) = result {
                    capture_text(&state, text.to_string());
                }
            },
        );
    }
}

fn capture_text(state: &State, text: String) {
    let decision = text_decision(&text, &state.last_text.borrow(), state.max_text_bytes);
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

    // Same change delivered twice → nothing to do.
    if *state.last_image_hash.borrow() == Some(hash) {
        let _ = std::fs::remove_file(&tmp_path);
        return;
    }
    // Store already contains this image
    if state.store.borrow().contains_image_hash(&hash) {
        let _ = std::fs::remove_file(&tmp_path);
        *state.last_image_hash.borrow_mut() = Some(hash);
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

    *state.last_image_hash.borrow_mut() = Some(hash);

    let id = state.store.borrow_mut().next_id();
    state.store.borrow_mut().add(ClipboardEntry::new_image(id, hash, w as u32, h as u32));
    tracing::debug!("[monitor] captured image {} ({w}×{h})", crate::paths::hex(&hash));
    (state.on_change)();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_capture_rules() {
        assert_eq!(text_decision("", "", 10), TextDecision::Ignore);
        assert_eq!(text_decision("same", "same", 10), TextDecision::Ignore);
        assert_eq!(text_decision("new", "old", 10), TextDecision::Capture);
        assert_eq!(text_decision("12345678901", "old", 10), TextDecision::TooLarge);
        assert_eq!(text_decision("1234567890", "old", 10), TextDecision::Capture);
    }
}
