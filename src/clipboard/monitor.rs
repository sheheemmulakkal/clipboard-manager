use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gdk4::prelude::*;
use glib::ControlFlow;

use crate::clipboard::entry::ClipboardEntry;
use crate::config::AppConfig;
use crate::store::Store;

const MAX_RAW_PIXELS: u64 = 3840 * 2160 * 4; // 4K cap (~33 MB raw)
const THUMB_W: i32 = 240;
const THUMB_H: i32 = 135;

pub struct ClipboardMonitor {
    #[allow(dead_code)]
    last_text: Rc<RefCell<String>>,
}

impl ClipboardMonitor {
    pub fn start(
        store:     Rc<RefCell<Box<dyn Store>>>,
        _config:   AppConfig,
        on_change: impl Fn() + 'static,
    ) -> Self {
        let last_text       = Rc::new(RefCell::new(String::new()));
        let last_image_hash: Rc<RefCell<Option<[u8; 32]>>> = Rc::new(RefCell::new(None));
        let on_change       = Rc::new(on_change);

        // Compute the images/ dir once at startup and ensure it exists.
        let image_dir: Rc<PathBuf> = Rc::new(
            dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("clipboard-manager")
                .join("images"),
        );
        if let Err(e) = std::fs::create_dir_all(image_dir.as_ref()) {
            tracing::warn!("[monitor] cannot create image dir: {e}");
        }

        // GDK clipboard is backend-agnostic: works on both X11 and Wayland.
        let clipboard = gdk4::Display::default()
            .expect("no GDK display")
            .clipboard();

        glib::timeout_add_local(Duration::from_millis(500), {
            let last_text       = Rc::clone(&last_text);
            let last_image_hash = Rc::clone(&last_image_hash);
            let store           = Rc::clone(&store);
            let on_change       = Rc::clone(&on_change);
            let image_dir       = Rc::clone(&image_dir);

            move || {
                let last_text       = Rc::clone(&last_text);
                let last_image_hash = Rc::clone(&last_image_hash);
                let store           = Rc::clone(&store);
                let on_change       = Rc::clone(&on_change);
                let image_dir       = Rc::clone(&image_dir);

                // Check what MIME types are available on the clipboard.
                let formats = clipboard.formats();
                let has_image = formats.contain_mime_type("image/png")
                    || formats.contain_mime_type("image/jpeg")
                    || formats.contain_mime_type("image/gif");
                let has_text = formats.contains_type(glib::types::Type::STRING)
                    || formats.contain_mime_type("text/plain")
                    || formats.contain_mime_type("text/plain;charset=utf-8");

                if has_image && !has_text {
                    // ── Image branch ──────────────────────────────────────
                    clipboard.read_texture_async(
                        None::<&gdk4::gio::Cancellable>,
                        move |result: Result<Option<gdk4::Texture>, glib::Error>| {
                            let texture = match result {
                                Ok(Some(t)) => t,
                                Ok(None) => {
                                    tracing::debug!("[monitor] clipboard returned no texture");
                                    return;
                                }
                                Err(e) => {
                                    tracing::debug!("[monitor] texture read error: {e}");
                                    return;
                                }
                            };

                            let w = texture.width();
                            let h = texture.height();
                            let raw_size = (w as u64) * (h as u64) * 4;
                            if raw_size > MAX_RAW_PIXELS {
                                tracing::warn!(
                                    "[monitor] image too large ({w}×{h}) — skipping"
                                );
                                return;
                            }

                            // Save to a temp file, compute SHA-256 from file bytes,
                            // then atomically rename to the final content-addressed path.
                            let tmp_path = image_dir.join("_capture.tmp");
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

                            #[cfg(feature = "sha2")]
                            let hash = {
                                use sha2::Digest;
                                let digest = sha2::Sha256::digest(&png_bytes);
                                let mut arr = [0u8; 32];
                                arr.copy_from_slice(&digest);
                                arr
                            };
                            #[cfg(not(feature = "sha2"))]
                            let hash = {
                                // Fallback: use a simple hash of the pixel data
                                // (sha2 is always enabled when ui feature is on, but
                                //  this keeps the code compilable without it)
                                let _ = png_bytes;
                                [0u8; 32]
                            };

                            // Dedup: skip if same hash as last captured image
                            {
                                let last = last_image_hash.borrow();
                                if *last == Some(hash) {
                                    let _ = std::fs::remove_file(&tmp_path);
                                    return;
                                }
                            }
                            // Dedup: skip if store already contains this image
                            if store.borrow().contains_image_hash(&hash) {
                                let _ = std::fs::remove_file(&tmp_path);
                                *last_image_hash.borrow_mut() = Some(hash);
                                return;
                            }

                            let hex: String = hash.iter()
                                .map(|b| format!("{b:02x}"))
                                .collect();

                            let full_path  = image_dir.join(format!("{hex}.png"));
                            let thumb_path = image_dir.join(format!("{hex}_thumb.png"));

                            // Rename tmp → final full image
                            if let Err(e) = std::fs::rename(&tmp_path, &full_path) {
                                tracing::warn!("[monitor] rename failed: {e}");
                                let _ = std::fs::remove_file(&tmp_path);
                                return;
                            }

                            // Generate thumbnail via gdk-pixbuf
                            #[cfg(feature = "gdk-pixbuf")]
                            {
                                match gdk_pixbuf::Pixbuf::from_file_at_scale(
                                    &full_path, THUMB_W, THUMB_H, true,
                                ) {
                                    Ok(pb) => {
                                        if let Err(e) = pb.savev(&thumb_path, "png", &[]) {
                                            tracing::warn!("[monitor] thumb save failed: {e}");
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!("[monitor] thumb scale failed: {e}");
                                    }
                                }
                            }

                            *last_image_hash.borrow_mut() = Some(hash);

                            let next_id = store.borrow().get_all().iter()
                                .map(|e| e.id)
                                .max()
                                .unwrap_or(0) + 1;
                            let entry = ClipboardEntry::new_image(
                                next_id, hash, w as u32, h as u32,
                            );
                            store.borrow_mut().add(entry);
                            tracing::debug!("[monitor] captured image {hex} ({w}×{h})");
                            on_change();
                        },
                    );
                } else {
                    // ── Text branch (unchanged) ───────────────────────────
                    clipboard.read_text_async(
                        None::<&gdk4::gio::Cancellable>,
                        move |result: Result<Option<glib::GString>, glib::Error>| {
                            if let Ok(Some(text)) = result {
                                let text = text.to_string();
                                if text.is_empty() {
                                    return;
                                }

                                let last = last_text.borrow().clone();
                                if text != last {
                                    *last_text.borrow_mut() = text.clone();

                                    let preview: String = text.chars().take(60).collect();
                                    tracing::debug!("[monitor] captured: {:?}", preview);
                                    let next_id = store
                                        .borrow()
                                        .get_all()
                                        .iter()
                                        .map(|e| e.id)
                                        .max()
                                        .unwrap_or(0)
                                        + 1;
                                    let entry = ClipboardEntry::new_text(next_id, text);
                                    store.borrow_mut().add(entry);

                                    on_change();
                                }
                            }
                        },
                    );
                }

                ControlFlow::Continue
            }
        });

        Self { last_text }
    }
}
