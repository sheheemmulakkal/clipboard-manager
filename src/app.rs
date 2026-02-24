use std::cell::RefCell;
use std::rc::Rc;

use anyhow::{anyhow, Result};
use gdk4::prelude::*;
use gtk4::Application;

use crate::clipboard::monitor::ClipboardMonitor;
use crate::clipboard::ClipboardEntry;
use crate::config::AppConfig;
use crate::hotkey::{HotkeyManager, X11HotkeyManager};
use crate::store::memory::MemoryStore;
use crate::store::Store;
use crate::tray::ClipboardTray;
use crate::ui::ClipboardPopup;

#[allow(dead_code)]
pub struct App {
    config: AppConfig,
    store:  Rc<RefCell<Box<dyn Store>>>,
}

impl App {
    pub fn new() -> Result<Self> {
        let config = AppConfig::load()?;
        if let Err(e) = Self::autostart_if_needed() {
            eprintln!("[autostart] warning: {e}");
        }
        let store: Box<dyn Store> =
            Box::new(MemoryStore::new(config.max_history, config.deduplicate));
        let store = Rc::new(RefCell::new(store));
        Ok(Self { config, store })
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
            eprintln!("[autostart] installed to {}", dest.display());
        }
        Ok(())
    }

    pub fn run(&self) -> Result<()> {
        tracing_subscriber::fmt::init();

        let app = Application::builder()
            .application_id("com.example.clipboard-manager")
            .build();

        let store      = Rc::clone(&self.store);
        let hotkey_str = self.config.hotkey.clone();

        app.connect_activate(move |app| {
            std::mem::forget(app.hold());

            let popup = Rc::new(ClipboardPopup::new(app));

            // ── Clipboard monitor ─────────────────────────────────────────
            let store_for_monitor = Rc::clone(&store);
            let store_for_cb      = Rc::clone(&store);
            let _monitor = ClipboardMonitor::start(
                store_for_monitor,
                AppConfig::default(),
                move || {
                    let count = store_for_cb.borrow().len();
                    eprintln!("[monitor] store now has {} item(s)", count);
                },
            );

            // ── Channels ──────────────────────────────────────────────────
            // hotkey thread  → GTK main thread (show popup)
            let (hotkey_tx, hotkey_rx)     = std::sync::mpsc::sync_channel::<()>(1);
            // tray "Show History" → GTK main thread (show popup)
            let (tray_show_tx, tray_show_rx) = std::sync::mpsc::sync_channel::<()>(1);
            // tray "Quit"        → GTK main thread (quit app)
            let (tray_quit_tx, tray_quit_rx) = std::sync::mpsc::sync_channel::<()>(1);

            // ── Global hotkey ─────────────────────────────────────────────
            let manager = X11HotkeyManager::new(&hotkey_str);
            if let Err(e) = manager.start(move || { let _ = hotkey_tx.try_send(()); }) {
                eprintln!("[hotkey] error (no global shortcut): {e}");
            }

            // ── System tray ───────────────────────────────────────────────
            let tray = ClipboardTray { show_tx: tray_show_tx, quit_tx: tray_quit_tx };
            let tray_handle = {
                use ksni::blocking::TrayMethods;
                match tray.spawn() {
                    Ok(h)  => Some(h),
                    Err(e) => { eprintln!("[tray] not available: {e}"); None }
                }
            };

            let popup_for_timer = Rc::clone(&popup);
            let store_for_timer = Rc::clone(&store);
            let app_for_quit    = app.clone();

            // ── 50 ms poll loop on GTK main thread ────────────────────────
            glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                // Keep alive: manager (XGrabKey) and tray handle (D-Bus)
                let _keep_manager = &manager;
                let _keep_tray    = &tray_handle;

                // Show popup: triggered by hotkey OR tray "Show History"
                if hotkey_rx.try_recv().is_ok() || tray_show_rx.try_recv().is_ok() {
                    let entries: Vec<ClipboardEntry> = store_for_timer
                        .borrow()
                        .get_all()
                        .into_iter()
                        .cloned()
                        .collect();

                    eprintln!("[hotkey] fired — store has {} item(s)", entries.len());

                    let popup_for_select = Rc::clone(&popup_for_timer);
                    popup_for_timer.populate(&entries, move |id, content| {
                        eprintln!("[select] item {id}");
                        if let Some(display) = gdk4::Display::default() {
                            display.clipboard().set_text(&content);
                        }
                        popup_for_select.hide();
                    });

                    popup_for_timer.show_at_cursor();
                }

                // Quit: triggered by tray "Quit"
                if tray_quit_rx.try_recv().is_ok() {
                    app_for_quit.quit();
                }

                glib::ControlFlow::Continue
            });
        });

        app.run();
        Ok(())
    }
}
