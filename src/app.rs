use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use gdk4::prelude::*;
use gtk4::Application;

use crate::clipboard::monitor::ClipboardMonitor;
use crate::clipboard::ClipboardEntry;
use crate::config::AppConfig;
use crate::hotkey;
use crate::platform;
#[cfg(not(feature = "persist"))]
use crate::store::memory::MemoryStore;
use crate::store::Store;
use crate::ui::ClipboardPopup;

#[allow(dead_code)]
pub struct App {
    config:         AppConfig,
    store:          Rc<RefCell<Box<dyn Store>>>,
    prev_window_id: Rc<Cell<Option<u64>>>,
}

impl App {
    pub fn new() -> Result<Self> {
        let config = AppConfig::load()?;

        if let Err(e) = Self::autostart_if_needed() {
            tracing::warn!("[autostart] {e}");
        }

        let store: Box<dyn Store> = {
            #[cfg(feature = "persist")]
            {
                use crate::store::persistent::PersistentStore;
                let path = dirs::data_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                    .join("clipboard-manager")
                    .join("history.bin");
                Box::new(PersistentStore::load(config.max_history, config.deduplicate, path))
            }
            #[cfg(not(feature = "persist"))]
            {
                Box::new(MemoryStore::new(config.max_history, config.deduplicate))
            }
        };
        let store = Rc::new(RefCell::new(store));

        Ok(Self { config, store, prev_window_id: Rc::new(Cell::new(None)) })
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
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
            )
            .init();

        let app = Application::builder()
            .application_id("com.example.clipboard-manager")
            .build();

        let store                   = Rc::clone(&self.store);
        let hotkey_str              = self.config.hotkey.clone();
        let popup_follow_cursor     = self.config.popup_follow_cursor;
        let clear_undo_timeout_secs = self.config.clear_undo_timeout_secs;
        let nerd_font               = self.config.nerd_font;
        let colors                  = self.config.colors.clone();
        let sizes                   = self.config.sizes.clone();
        let prev_window_id          = Rc::clone(&self.prev_window_id);

        // ── Single-instance re-activation ─────────────────────────────────────
        // GTK enforces a single instance via D-Bus (application_id).
        // When a second `clipboard-manager` binary is launched (e.g. from the
        // GNOME keyboard shortcut), GTK routes it to the already-running instance
        // by calling connect_activate again. We detect this with `first_run` and
        // send to `show_tx` to open the popup — no signals or pkill needed.
        let first_run = Arc::new(AtomicBool::new(true));
        let first_run_flag = Arc::clone(&first_run);
        let show_tx: Arc<Mutex<Option<std::sync::mpsc::SyncSender<Option<u64>>>>> =
            Arc::new(Mutex::new(None));
        let show_tx_reactivate = Arc::clone(&show_tx);

        app.connect_activate(move |app| {
            if !first_run_flag.swap(false, Ordering::SeqCst) {
                // Re-activation: another instance was launched → show the popup.
                if let Some(tx) = show_tx_reactivate.lock().unwrap().as_ref() {
                    let _ = tx.try_send(None);
                }
                return;
            }

            std::mem::forget(app.hold());

            // ── Platform detection (Strategy pattern) ─────────────────────
            // Returns Arc<dyn Platform> — X11Platform or WaylandPlatform
            // selected at runtime by inspecting $WAYLAND_DISPLAY.
            let platform = platform::detect();

            let prev_window_id = Rc::clone(&prev_window_id);
            let store          = Rc::clone(&store);

            let popup = Rc::new(ClipboardPopup::new(app, Arc::clone(&platform), nerd_font, &colors, &sizes));

            // ── Search state ──────────────────────────────────────────────────
            let search_query: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
            let repop_shared: Rc<RefCell<Option<Box<dyn Fn()>>>> = Rc::new(RefCell::new(None));

            popup.connect_search_changed({
                let sq = Rc::clone(&search_query);
                let rs = Rc::clone(&repop_shared);
                move |q| {
                    *sq.borrow_mut() = q;
                    if let Some(f) = rs.borrow().as_ref() { f(); }
                }
            });

            // ── Clipboard monitor ─────────────────────────────────────────
            let store_for_monitor = Rc::clone(&store);
            let store_for_cb      = Rc::clone(&store);
            let _monitor = ClipboardMonitor::start(
                store_for_monitor,
                AppConfig::default(),
                move || {
                    let count = store_for_cb.borrow().len();
                    tracing::debug!("[monitor] store now has {} item(s)", count);
                },
            );

            // ── Channels ──────────────────────────────────────────────────
            let (hotkey_tx, hotkey_rx) = std::sync::mpsc::sync_channel::<Option<u64>>(1);

            // Expose hotkey_tx to the re-activation handler registered above.
            *show_tx.lock().unwrap() = Some(hotkey_tx.clone());

            // ── Global hotkey ─────────────────────────────────────────────
            // Hotkey backend is chosen by hotkey::detect() — X11HotkeyManager
            // on X11 (XGrabKey), WaylandHotkeyManager on Wayland (portal).
            // The closure captures platform via Arc so capture_active_window()
            // can be called from the background hotkey thread.
            let hotkey_manager = hotkey::detect(&hotkey_str);
            let platform_hk    = Arc::clone(&platform);

            match hotkey_manager.start(Box::new(move || {
                // Called from the hotkey background thread.
                // Capture the window that had focus *before* the popup opens.
                // X11: queries _NET_ACTIVE_WINDOW via x11rb.
                // Wayland: returns None (compositor doesn't expose this).
                let prev = platform_hk.capture_active_window();
                tracing::debug!("[hotkey] captured prev_window={prev:?}");
                let _ = hotkey_tx.try_send(prev);
            })) {
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

            let popup_for_timer = Rc::clone(&popup);
            let store_for_timer = Rc::clone(&store);

            // ── 50 ms poll loop ───────────────────────────────────────────
            glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                let _keep_manager = &hotkey_manager;

                let show_trigger: Option<Option<u64>> = hotkey_rx.try_recv().ok();

                if let Some(prev_win) = show_trigger {
                    prev_window_id.set(prev_win);

                    // Clear search on each popup open
                    let search_query_open = Rc::clone(&search_query);
                    let repop_shared_open = Rc::clone(&repop_shared);
                    *search_query_open.borrow_mut() = String::new();
                    popup_for_timer.clear_search();

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

                    // Clone platform for use inside the repopulate closure.
                    let platform_inner = Arc::clone(&platform);

                    let sq_r = Rc::clone(&search_query_open);
                    *repopulate.borrow_mut() = Some(Box::new(move || {
                        let sorted  = sorted_entries(&store_r.borrow());
                        let query   = sq_r.borrow().clone();
                        let entries = filter_entries(sorted, &query);

                        let store_sel  = Rc::clone(&store_r);
                        let popup_sel  = Rc::clone(&popup_r);
                        let cell_sel   = Rc::clone(&cell_r);

                        let store_rm   = Rc::clone(&store_r);
                        let repop_rm   = Rc::clone(&repop_r);

                        let store_pin  = Rc::clone(&store_r);
                        let repop_pin  = Rc::clone(&repop_r);

                        let store_lbl  = Rc::clone(&store_r);
                        let repop_lbl  = Rc::clone(&repop_r);

                        let store_clr  = Rc::clone(&store_r);
                        let repop_clr  = Rc::clone(&repop_r);
                        let popup_clr  = Rc::clone(&popup_r);

                        // Platform clones for paste callbacks.
                        let platform_sel = Arc::clone(&platform_inner);
                        let platform_tp  = Arc::clone(&platform_inner);

                        let cell_tp  = Rc::clone(&cell_r);
                        let popup_tp = Rc::clone(&popup_r);
                        let store_tp = Rc::clone(&store_r);

                        popup_r.populate(
                            &entries,
                            // ── on_select: copy + hide + paste ────────────
                            move |_id, content| {
                                if let Some(display) = gdk4::Display::default() {
                                    display.clipboard().set_text(&content);
                                }
                                popup_sel.hide();

                                let prev_id    = cell_sel.get();
                                let plat_paste = Arc::clone(&platform_sel);
                                tracing::debug!("[select] prev_id={prev_id:?}");

                                // Delay so the popup fully hides and the
                                // previous window regains focus before paste.
                                glib::timeout_add_local_once(
                                    std::time::Duration::from_millis(200),
                                    move || {
                                        std::thread::spawn(move || {
                                            // X11: activate prev_window + XTest Ctrl+V
                                            // Wayland: RemoteDesktop portal Ctrl+V
                                            plat_paste.paste(prev_id);
                                        });
                                    },
                                );

                                let _ = &store_sel;
                            },
                            // ── on_copy: copy to clipboard only (no paste) ─
                            move |_id, content| {
                                if let Some(display) = gdk4::Display::default() {
                                    display.clipboard().set_text(&content);
                                }
                                tracing::debug!("[copy] copied to clipboard (no paste)");
                            },
                            // ── on_terminal_paste: copy + hide + Ctrl+Shift+V
                            move |_id, content| {
                                if let Some(display) = gdk4::Display::default() {
                                    display.clipboard().set_text(&content);
                                }
                                popup_tp.hide();

                                let prev_id    = cell_tp.get();
                                let plat_paste = Arc::clone(&platform_tp);
                                tracing::debug!("[terminal_paste] prev_id={prev_id:?}");

                                glib::timeout_add_local_once(
                                    std::time::Duration::from_millis(200),
                                    move || {
                                        std::thread::spawn(move || {
                                            plat_paste.paste_terminal(prev_id);
                                        });
                                    },
                                );

                                let _ = &store_tp;
                            },
                            // ── on_remove ─────────────────────────────────
                            move |id| {
                                store_rm.borrow_mut().remove(id);
                                tracing::debug!("[remove] id={id}");
                                if let Some(f) = repop_rm.borrow().as_ref() { f(); }
                            },
                            // ── on_pin ────────────────────────────────────
                            move |id, pinned| {
                                store_pin.borrow_mut().set_pinned(id, pinned);
                                tracing::debug!("[pin] id={id} pinned={pinned}");
                                if let Some(f) = repop_pin.borrow().as_ref() { f(); }
                            },
                            // ── on_label ──────────────────────────────────
                            move |id, label, color| {
                                store_lbl.borrow_mut().set_label(id, label, color);
                                tracing::debug!("[label] id={id}");
                                if let Some(f) = repop_lbl.borrow().as_ref() { f(); }
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
                                tracing::debug!("[clear] pending undo for {count} item(s)");

                                let store_commit = Rc::clone(&store_clr);
                                let repop_commit = Rc::clone(&repop_clr);
                                let repop_undo   = Rc::clone(&repop_clr);

                                popup_clr.show_undo_bar(
                                    count,
                                    clear_undo_timeout_secs,
                                    // on_undo: store untouched → just repopulate
                                    move || {
                                        tracing::debug!("[clear] undone");
                                        if let Some(f) = repop_undo.borrow().as_ref() { f(); }
                                    },
                                    // on_commit: now actually clear + repopulate
                                    move || {
                                        store_commit.borrow_mut().clear_unpinned();
                                        tracing::debug!("[clear] committed");
                                        if let Some(f) = repop_commit.borrow().as_ref() { f(); }
                                    },
                                );
                            },
                        );
                    }));

                    // Point repop_shared at this popup session's repopulate closure
                    let repop_for_shared = Rc::clone(&repopulate);
                    *repop_shared_open.borrow_mut() = Some(Box::new(move || {
                        if let Some(f) = repop_for_shared.borrow().as_ref() { f(); }
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

    all.sort_by(|a, b| {
        match (a.pinned, b.pinned) {
            (true, false)  => std::cmp::Ordering::Less,
            (false, true)  => std::cmp::Ordering::Greater,
            _              => b.copied_at.cmp(&a.copied_at),
        }
    });

    all
}

/// Filter entries by a case-insensitive substring match on content or label.
/// Returns all entries unchanged when `query` is empty.
fn filter_entries(entries: Vec<ClipboardEntry>, query: &str) -> Vec<ClipboardEntry> {
    if query.is_empty() { return entries; }
    let q = query.to_lowercase();
    entries.into_iter()
        .filter(|e| {
            e.content.to_lowercase().contains(&q)
                || e.label.as_deref()
                       .map(|l| l.to_lowercase().contains(&q))
                       .unwrap_or(false)
        })
        .collect()
}
