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

fn should_write_autostart(user_entry: bool, system_entry: bool, written_before: bool) -> bool {
    !user_entry && !system_entry && !written_before
}

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
        gc_image_files(&image_dir, store.borrow().as_ref());

        Ok(Self { config, store, config_error })
    }

    /// Write `~/.config/autostart/clipboard-manager.desktop` once, for installs
    /// without the system-wide entry (e.g. `cargo install`). Never re-created
    /// after the user deletes it, so autostart can be turned off.
    fn autostart_if_needed() -> Result<()> {
        const SYSTEM_ENTRY: &str = "/etc/xdg/autostart/clipboard-manager.desktop";
        let marker = crate::paths::state_dir().join("autostart-installed");
        let autostart_dir = dirs::config_dir()
            .ok_or_else(|| anyhow!("no config dir"))?
            .join("autostart");
        let dest = autostart_dir.join("clipboard-manager.desktop");
        if !should_write_autostart(
            dest.exists(),
            std::path::Path::new(SYSTEM_ENTRY).exists(),
            marker.exists(),
        ) {
            return Ok(());
        }
        std::fs::create_dir_all(&autostart_dir)?;
        let exe = std::env::current_exe()?;
        let content = format!(
            "[Desktop Entry]\nType=Application\nName=Clipboard Manager\n\
             Comment=Clipboard history popup\nExec={}\nIcon=edit-paste\n\
             Terminal=false\nCategories=Utility;\nStartupNotify=false\n",
            exe.display()
        );
        std::fs::write(&dest, content)?;
        std::fs::write(&marker, b"")?;
        tracing::debug!("[autostart] installed to {}", dest.display());
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
            .application_id(crate::paths::application_id())
            .flags(gtk4::gio::ApplicationFlags::HANDLES_COMMAND_LINE)
            .build();

        let store        = Rc::clone(&self.store);
        let config       = self.config.clone();
        let config_error = self.config_error.clone();

        // Background threads → main loop.
        let (tx, rx) = async_channel::unbounded::<AppEvent>();

        // ── Single instance + command line ────────────────────────────────────
        // GTK enforces a single instance via D-Bus (application_id). A second
        // `clipboard-manager [command]` forwards its command line here; the
        // first (local) command line is this daemon's own start.
        let controller_slot: Rc<RefCell<Option<Rc<Controller>>>> = Rc::new(RefCell::new(None));
        {
            let slot = Rc::clone(&controller_slot);
            app.connect_command_line(move |app, cmdline| {
                if !cmdline.is_remote() {
                    app.activate();
                    return 0;
                }
                let args: Vec<String> = cmdline
                    .arguments()
                    .iter()
                    .map(|a| a.to_string_lossy().into_owned())
                    .collect();
                tracing::debug!("[cli] remote command line: {:?}", &args[1..]);
                let controller = slot.borrow().clone();
                // (Output can't be printed into the caller's terminal before
                // glib 2.80, so remote commands report via the exit code.)
                let ok = match (crate::cli::parse(&args), controller) {
                    (Ok(cmd), Some(c)) => c.run_command(cmd),
                    _ => false,
                };
                if ok { 0 } else { 1 }
            });
        }

        let activated = Cell::new(false);
        let slot = Rc::clone(&controller_slot);
        app.connect_activate(move |app| {
            if activated.replace(true) {
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
            *slot.borrow_mut() = Some(Rc::clone(&controller));
            // Started by `clipboard-manager show` while no instance was running.
            if std::env::var_os("_CM_SHOW_ON_START").is_some() {
                let _ = tx.send_blocking(AppEvent::Show { prev_window: None });
            }

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
fn gc_image_files(image_dir: &std::path::Path, store: &dyn Store) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autostart_written_only_once_and_not_over_system_entry() {
        assert!(should_write_autostart(false, false, false));
        assert!(!should_write_autostart(true, false, false)); // user file exists
        assert!(!should_write_autostart(false, true, false)); // system entry (.deb)
        assert!(!should_write_autostart(false, false, true)); // written before, user deleted it
    }
}
