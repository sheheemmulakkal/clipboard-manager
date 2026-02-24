use std::cell::{Cell, RefCell};
use std::rc::Rc;

use anyhow::{anyhow, Result};
use gdk4::prelude::*;
use gtk4::Application;

use crate::clipboard::monitor::ClipboardMonitor;
use crate::clipboard::ClipboardEntry;
use crate::config::AppConfig;
use crate::hotkey::{HotkeyManager, X11HotkeyManager};
use crate::paste::Paster;
use crate::store::memory::MemoryStore;
use crate::store::Store;
use crate::tray::ClipboardTray;
use crate::ui::ClipboardPopup;

#[allow(dead_code)]
pub struct App {
    config:          AppConfig,
    store:           Rc<RefCell<Box<dyn Store>>>,
    prev_window_id:  Rc<Cell<Option<u64>>>,
    xdotool_ok:      bool,
}

impl App {
    pub fn new() -> Result<Self> {
        let config = AppConfig::load()?;

        // Task G — check xdotool at startup
        let xdotool_ok = std::process::Command::new("xdotool")
            .arg("version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !xdotool_ok {
            tracing::warn!(
                "xdotool not found. Paste feature disabled. \
                 Install with: sudo apt install xdotool"
            );
        }

        if let Err(e) = Self::autostart_if_needed() {
            eprintln!("[autostart] warning: {e}");
        }

        let store: Box<dyn Store> =
            Box::new(MemoryStore::new(config.max_history, config.deduplicate));
        let store = Rc::new(RefCell::new(store));

        Ok(Self {
            config,
            store,
            prev_window_id: Rc::new(Cell::new(None)),
            xdotool_ok,
        })
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

        let store          = Rc::clone(&self.store);
        let hotkey_str     = self.config.hotkey.clone();
        let xdotool_ok     = self.xdotool_ok;
        let prev_window_id = Rc::clone(&self.prev_window_id);

        app.connect_activate(move |app| {
            std::mem::forget(app.hold());

            // Re-clone Rc handles inside the Fn closure so the move-closures
            // below can capture the local clones rather than the outer captures
            // (Fn closures cannot move out their captured variables).
            let prev_window_id = Rc::clone(&prev_window_id);
            let store          = Rc::clone(&store);

            let popup = Rc::new(ClipboardPopup::new(app));

            // ── Clipboard monitor ─────────────────────────────────────────
            let store_for_monitor = Rc::clone(&store);
            let store_for_cb = Rc::clone(&store);
            let _monitor = ClipboardMonitor::start(
                store_for_monitor,
                AppConfig::default(),
                move || {
                    let count = store_for_cb.borrow().len();
                    eprintln!("[monitor] store now has {} item(s)", count);
                },
            );

            // ── Channels ──────────────────────────────────────────────────
            // Task A: hotkey channel carries Option<u64> (prev window ID).
            // The callback captures the active window BEFORE the popup opens.
            let (hotkey_tx, hotkey_rx)       = std::sync::mpsc::sync_channel::<Option<u64>>(1);
            let (tray_show_tx, tray_show_rx) = std::sync::mpsc::sync_channel::<()>(1);
            let (tray_quit_tx, tray_quit_rx) = std::sync::mpsc::sync_channel::<()>(1);

            // ── Global hotkey ─────────────────────────────────────────────
            let manager = X11HotkeyManager::new(&hotkey_str);
            if let Err(e) = manager.start(move || {
                // Task A: capture the active window BEFORE sending the signal.
                let prev = std::process::Command::new("xdotool")
                    .arg("getactivewindow")
                    .output()
                    .ok()
                    .and_then(|o| String::from_utf8(o.stdout).ok())
                    .and_then(|s| s.trim().parse::<u64>().ok());
                eprintln!("[hotkey] captured prev_window={prev:?}");
                let _ = hotkey_tx.try_send(prev);
            }) {
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
                let _keep_manager = &manager;
                let _keep_tray    = &tray_handle;

                // Determine show trigger and its associated prev-window ID.
                // Hotkey carries Option<u64>; tray "Show History" yields None.
                let show_trigger: Option<Option<u64>> = hotkey_rx
                    .try_recv()
                    .ok()
                    .or_else(|| tray_show_rx.try_recv().ok().map(|()| None));

                if let Some(prev_win) = show_trigger {
                    // Store for on_select to read later (Task E).
                    prev_window_id.set(prev_win);

                    let entries: Vec<ClipboardEntry> = store_for_timer
                        .borrow()
                        .get_all()
                        .into_iter()
                        .cloned()
                        .collect();

                    eprintln!("[hotkey] fired — store has {} item(s)", entries.len());

                    // Clone the cell handle so on_select can read prev_win.
                    let cell_for_select  = Rc::clone(&prev_window_id);
                    let popup_for_select = Rc::clone(&popup_for_timer);

                    // Task D — on_select: set clipboard → hide → schedule paste
                    popup_for_timer.populate(&entries, move |_id, content| {
                        if let Some(display) = gdk4::Display::default() {
                            display.clipboard().set_text(&content);
                        }

                        popup_for_select.hide();

                        let prev_id = cell_for_select.get();
                        eprintln!("[select] xdotool_ok={xdotool_ok} prev_id={prev_id:?}");

                        if xdotool_ok {
                            // 200ms: enough for popup to close + clipboard to settle.
                            // Paste runs in a thread to avoid sleeping on the GTK main loop.
                            glib::timeout_add_local_once(
                                std::time::Duration::from_millis(200),
                                move || {
                                    std::thread::spawn(move || {
                                        let result = match prev_id {
                                            Some(win_id) => Paster::paste_into_window(win_id),
                                            None         => Paster::paste_into_focused(),
                                        };
                                        if let Err(e) = result {
                                            eprintln!("[paste] failed: {e}");
                                        }
                                    });
                                },
                            );
                        }
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
