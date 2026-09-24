use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use gtk4::prelude::*;
use gtk4::Application;

use crate::clipboard::entry::ClipboardContent;
use crate::clipboard::monitor::ClipboardMonitor;
use crate::config::AppConfig;
use crate::controller::Controller;
use crate::events::AppEvent;
use crate::hotkey;
use crate::paths;
use crate::platform;
#[cfg(not(feature = "persist"))]
use crate::store::memory::MemoryStore;
use crate::store::Store;
use crate::ui::ClipboardPopup;

pub struct App {
    config:       AppConfig,
    store:        Rc<RefCell<Box<dyn Store>>>,
    config_error: Option<String>,
}

impl App {
    pub fn new() -> Result<Self> {
        let (config, config_error) = AppConfig::load();

        if let Err(e) = Self::autostart_if_needed() {
            tracing::warn!("[autostart] {e}");
        }

        let store: Box<dyn Store> = {
            #[cfg(feature = "persist")]
            {
                use crate::store::persistent::PersistentStore;
                Box::new(PersistentStore::load(config.max_history, config.deduplicate, paths::history_file()))
            }
            #[cfg(not(feature = "persist"))]
            {
                Box::new(MemoryStore::new(config.max_history, config.deduplicate))
            }
        };
        let store = Rc::new(RefCell::new(store));

        let image_dir = paths::image_dir();

        // Startup GC: delete image files not referenced by any current store entry.
        gc_image_files(&image_dir, &store.borrow());

        Ok(Self { config, store, config_error })
    }

    fn autostart_if_needed() -> Result<()> {
        let autostart_dir = dirs::config_dir()
            .ok_or_else(|| anyhow!("no config dir"))?
            .join("autostart");
        let dest = autostart_dir.join("clipboard-manager.desktop");
        if !dest.exists() {
            std::fs::create_dir_all(&autostart_dir)?;
            let exe = std::env::current_exe()?;
            let content = format!(
                "[Desktop Entry]\nType=Application\nName=Clipboard Manager\n\
                 Comment=Clipboard history popup\nExec={}\nIcon=edit-paste\n\
                 Terminal=false\nCategories=Utility;\nStartupNotify=false\n",
                exe.display()
            );
            std::fs::write(&dest, content)?;
            tracing::debug!("[autostart] installed to {}", dest.display());
        }
        Ok(())
    }

    pub fn run(&self) -> Result<()> {
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .with_ansi(std::env::var_os("_CM_DAEMON").is_none())
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
            )
            .init();

        let app = Application::builder()
            .application_id("com.example.clipboard-manager")
            .build();

        let store        = Rc::clone(&self.store);
        let config       = self.config.clone();
        let config_error = self.config_error.clone();

        // Background threads → main loop.
        let (tx, rx) = async_channel::unbounded::<AppEvent>();

        // ── Single-instance re-activation ─────────────────────────────────────
        // GTK enforces a single instance via D-Bus (application_id). When a
        // second `clipboard-manager` is launched (e.g. from a keyboard
        // shortcut), GTK routes it here by calling connect_activate again.
        let first_run = Cell::new(true);

        app.connect_activate(move |app| {
            if !first_run.replace(false) {
                let _ = tx.send_blocking(AppEvent::Show { prev_window: None });
                return;
            }

            std::mem::forget(app.hold());

            if let Some(msg) = &config_error {
                crate::notify::error("Clipboard Manager: config error", &format!("{msg}\nUsing default settings."));
            }

            // ── Platform detection (Strategy pattern) ─────────────────────
            let platform = platform::detect();

            let popup = ClipboardPopup::new(app, Arc::clone(&platform), &config);
            let paused = Rc::new(Cell::new(false));
            let controller = Controller::new(
                config.clone(), Rc::clone(&store), popup, Arc::clone(&platform), Rc::clone(&paused),
            );

            // ── Clipboard monitor ─────────────────────────────────────────
            let store_for_cb = Rc::clone(&store);
            let _monitor = ClipboardMonitor::start(Rc::clone(&store), &config, Arc::clone(&platform), paused, move || {
                tracing::debug!("[monitor] store now has {} item(s)", store_for_cb.borrow().len());
            });

            // ── Global hotkey ─────────────────────────────────────────────
            // X11HotkeyManager (XGrabKey) or WaylandHotkeyManager (portal).
            // The callback runs on the hotkey thread: capture the window that
            // had focus *before* the popup opens, then hand over to main.
            let hotkey_manager = hotkey::detect(&config.hotkey);
            let platform_hk    = Arc::clone(&platform);
            let tx_hk          = tx.clone();
            match hotkey_manager.start(Box::new(move || {
                let prev = platform_hk.capture_active_window();
                tracing::debug!("[hotkey] captured prev_window={prev:?}");
                let _ = tx_hk.send_blocking(AppEvent::Show { prev_window: prev });
            })) {
                Ok(()) => tracing::info!(
                    "Hotkey registered: {} — press it to open clipboard history",
                    config.hotkey
                ),
                Err(e) => crate::notify::error(
                    "Clipboard Manager: hotkey not registered",
                    &format!("'{}': {e:#}\nExample: hotkey = \"ctrl+alt+v\" in config.toml", config.hotkey),
                ),
            }

            controller.start_expiry();

            // ── Tray icon ─────────────────────────────────────────────────
            if config.tray_icon {
                if let Some(handle) = crate::tray::spawn(tx.clone(), &config.hotkey) {
                    controller.on_pause_changed(move |p| crate::tray::set_paused(&handle, p));
                }
            }

            // ── Event loop: background threads → controller ───────────────
            let rx = rx.clone();
            glib::spawn_future_local(async move {
                let _keep_hotkey = hotkey_manager;
                while let Ok(ev) = rx.recv().await {
                    controller.handle_app(ev);
                }
            });
        });

        app.run();
        Ok(())
    }
}

/// Startup GC: delete image files in `image_dir` whose hash is not in the store.
fn gc_image_files(image_dir: &std::path::Path, store: &Box<dyn Store>) {
    use std::collections::HashSet;
    let hashes: HashSet<String> = store.get_all().iter()
        .filter_map(|e| {
            if let ClipboardContent::Image { hash, .. } = &e.content {
                Some(paths::hex(hash))
            } else {
                None
            }
        })
        .collect();

    let rd = match std::fs::read_dir(image_dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        // Derive base hash from filename: strip _thumb.png or .png suffix
        let base = if let Some(s) = name.strip_suffix("_thumb.png") {
            s
        } else if let Some(s) = name.strip_suffix(".png") {
            s
        } else {
            continue;
        };
        if !hashes.contains(base) {
            tracing::debug!("[gc] removing orphaned image file: {name}");
            let _ = std::fs::remove_file(entry.path());
        }
    }
}
