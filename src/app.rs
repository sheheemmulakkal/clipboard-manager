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
    config:         AppConfig,
    store:          Rc<RefCell<Box<dyn Store>>>,
    prev_window_id: Rc<Cell<Option<u64>>>,
    xdotool_ok:     bool,
}

impl App {
    pub fn new() -> Result<Self> {
        let config = AppConfig::load()?;

        let xdotool_ok = std::process::Command::new("xdotool")
            .arg("version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !xdotool_ok {
            tracing::warn!(
                "xdotool not found. Paste / window-move features disabled. \
                 Install with: sudo apt install xdotool"
            );
        }

        if let Err(e) = Self::autostart_if_needed() {
            eprintln!("[autostart] warning: {e}");
        }

        let store: Box<dyn Store> =
            Box::new(MemoryStore::new(config.max_history, config.deduplicate));
        let store = Rc::new(RefCell::new(store));

        Ok(Self { config, store, prev_window_id: Rc::new(Cell::new(None)), xdotool_ok })
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

        let store                  = Rc::clone(&self.store);
        let hotkey_str             = self.config.hotkey.clone();
        let xdotool_ok             = self.xdotool_ok;
        let popup_follow_cursor    = self.config.popup_follow_cursor;
        let clear_undo_timeout_secs = self.config.clear_undo_timeout_secs;
        let prev_window_id         = Rc::clone(&self.prev_window_id);

        app.connect_activate(move |app| {
            std::mem::forget(app.hold());

            let prev_window_id = Rc::clone(&prev_window_id);
            let store          = Rc::clone(&store);

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
            let (hotkey_tx, hotkey_rx)       = std::sync::mpsc::sync_channel::<Option<u64>>(1);
            let (tray_show_tx, tray_show_rx) = std::sync::mpsc::sync_channel::<()>(1);
            let (tray_quit_tx, tray_quit_rx) = std::sync::mpsc::sync_channel::<()>(1);

            // ── Global hotkey ─────────────────────────────────────────────
            let manager = X11HotkeyManager::new(&hotkey_str);
            match manager.start(move || {
                let prev = std::process::Command::new("xdotool")
                    .arg("getactivewindow")
                    .output()
                    .ok()
                    .and_then(|o| String::from_utf8(o.stdout).ok())
                    .and_then(|s| s.trim().parse::<u64>().ok());
                eprintln!("[hotkey] captured prev_window={prev:?}");
                let _ = hotkey_tx.try_send(prev);
            }) {
                Ok(()) => tracing::info!(
                    "Hotkey registered: {} — press it to open clipboard history",
                    hotkey_str
                ),
                Err(e) => {
                    eprintln!("ERROR: Invalid hotkey '{}' in config.toml", hotkey_str);
                    eprintln!("  Reason: {e}");
                    eprintln!("  Valid example: hotkey = \"ctrl+alt+v\"");
                    eprintln!("  See ~/.config/clipboard-manager/config.toml");
                    std::process::exit(1);
                }
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

            // ── 50 ms poll loop ───────────────────────────────────────────
            glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                let _keep_manager = &manager;
                let _keep_tray    = &tray_handle;

                let show_trigger: Option<Option<u64>> = hotkey_rx
                    .try_recv()
                    .ok()
                    .or_else(|| tray_show_rx.try_recv().ok().map(|()| None));

                if let Some(prev_win) = show_trigger {
                    prev_window_id.set(prev_win);

                    // ── The "repopulate" closure ──────────────────────────
                    // Stored in Rc<RefCell<Option<Box<dyn Fn()>>>> so that
                    // on_remove / on_pin callbacks can call it after mutating
                    // the store, without holding a conflicting borrow.
                    let repopulate: Rc<RefCell<Option<Box<dyn Fn()>>>> =
                        Rc::new(RefCell::new(None));

                    let store_r  = Rc::clone(&store_for_timer);
                    let popup_r  = Rc::clone(&popup_for_timer);
                    let cell_r   = Rc::clone(&prev_window_id);
                    let repop_r  = Rc::clone(&repopulate);

                    *repopulate.borrow_mut() = Some(Box::new(move || {
                        let entries = sorted_entries(&store_r.borrow());

                        let store_sel  = Rc::clone(&store_r);
                        let popup_sel  = Rc::clone(&popup_r);
                        let cell_sel   = Rc::clone(&cell_r);

                        let store_rm    = Rc::clone(&store_r);
                        let repop_rm    = Rc::clone(&repop_r);

                        let store_pin   = Rc::clone(&store_r);
                        let repop_pin   = Rc::clone(&repop_r);

                        let store_clr   = Rc::clone(&store_r);
                        let repop_clr   = Rc::clone(&repop_r);
                        let popup_clr   = Rc::clone(&popup_r);

                        popup_r.populate(
                            &entries,
                            // ── on_select ─────────────────────────────────
                            move |_id, content| {
                                if let Some(display) = gdk4::Display::default() {
                                    display.clipboard().set_text(&content);
                                }
                                popup_sel.hide();

                                let prev_id = cell_sel.get();
                                eprintln!("[select] xdotool_ok={xdotool_ok} prev_id={prev_id:?}");

                                if xdotool_ok {
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
                                let _ = &store_sel;
                            },
                            // ── on_remove ─────────────────────────────────
                            move |id| {
                                store_rm.borrow_mut().remove(id);
                                eprintln!("[remove] id={id}");
                                if let Some(f) = repop_rm.borrow().as_ref() { f(); }
                            },
                            // ── on_pin ────────────────────────────────────
                            move |id, pinned| {
                                store_pin.borrow_mut().set_pinned(id, pinned);
                                eprintln!("[pin] id={id} pinned={pinned}");
                                if let Some(f) = repop_pin.borrow().as_ref() { f(); }
                            },
                            // ── on_clear ──────────────────────────────────
                            // Does NOT clear the store yet — delegates to
                            // show_undo_bar which calls on_commit only after
                            // the timeout (or popup close) and on_undo to restore.
                            move || {
                                let count = store_clr
                                    .borrow()
                                    .get_all()
                                    .iter()
                                    .filter(|e| !e.pinned)
                                    .count();

                                if count == 0 { return; }
                                eprintln!("[clear] pending undo for {count} item(s)");

                                let store_commit = Rc::clone(&store_clr);
                                let repop_commit = Rc::clone(&repop_clr);
                                let repop_undo   = Rc::clone(&repop_clr);

                                popup_clr.show_undo_bar(
                                    count,
                                    clear_undo_timeout_secs,
                                    // on_undo: store untouched → just repopulate
                                    move || {
                                        eprintln!("[clear] undone");
                                        if let Some(f) = repop_undo.borrow().as_ref() { f(); }
                                    },
                                    // on_commit: now actually clear + repopulate
                                    move || {
                                        store_commit.borrow_mut().clear_unpinned();
                                        eprintln!("[clear] committed");
                                        if let Some(f) = repop_commit.borrow().as_ref() { f(); }
                                    },
                                );
                            },
                        );
                    }));

                    // Trigger first populate + show
                    if let Some(f) = repopulate.borrow().as_ref() {
                        f();
                    }
                    if popup_follow_cursor {
                        popup_for_timer.show_at_cursor();
                    } else {
                        popup_for_timer.show_centered();
                    }
                }

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

/// Return entries sorted: pinned items first (by copied_at desc),
/// then unpinned items (newest first).
fn sorted_entries(store: &Box<dyn Store>) -> Vec<ClipboardEntry> {
    let mut all: Vec<ClipboardEntry> = store.get_all().into_iter().cloned().collect();

    // Sort: pinned first, then by copied_at descending within each group
    all.sort_by(|a, b| {
        match (a.pinned, b.pinned) {
            (true, false)  => std::cmp::Ordering::Less,
            (false, true)  => std::cmp::Ordering::Greater,
            _              => b.copied_at.cmp(&a.copied_at),
        }
    });

    all
}
