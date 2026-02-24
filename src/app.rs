use std::cell::RefCell;
use std::rc::Rc;

use anyhow::Result;
use gdk4::prelude::*;
use gtk4::Application;

use crate::clipboard::monitor::ClipboardMonitor;
use crate::clipboard::ClipboardEntry;
use crate::config::AppConfig;
use crate::hotkey::{HotkeyManager, X11HotkeyManager};
use crate::store::memory::MemoryStore;
use crate::store::Store;
use crate::ui::ClipboardPopup;

#[allow(dead_code)]
pub struct App {
    config: AppConfig,
    store:  Rc<RefCell<Box<dyn Store>>>,
}

impl App {
    pub fn new() -> Result<Self> {
        let config = AppConfig::load()?;
        let store: Box<dyn Store> =
            Box::new(MemoryStore::new(config.max_history, config.deduplicate));
        let store = Rc::new(RefCell::new(store));
        Ok(Self { config, store })
    }

    pub fn run(&self) -> Result<()> {
        tracing_subscriber::fmt::init();

        let app = Application::builder()
            .application_id("com.example.clipboard-manager")
            .build();

        let store      = Rc::clone(&self.store);
        let hotkey_str = self.config.hotkey.clone(); // e.g. "ctrl+alt+c"

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
                    println!("Clipboard changed, {} items", count);
                },
            );

            // ── Global hotkey ─────────────────────────────────────────────
            // on_hotkey must be Send; GTK types are not Send, so we bridge
            // via mpsc: the callback just pings a channel, and a glib timer
            // on the main thread picks it up and drives the popup.
            let (tx, rx) = std::sync::mpsc::sync_channel::<()>(1);

            let manager = X11HotkeyManager::new(&hotkey_str);
            if let Err(e) = manager.start(move || { let _ = tx.try_send(()); }) {
                eprintln!("Hotkey error (running without global shortcut): {}", e);
            }

            let popup_for_hotkey = Rc::clone(&popup);
            let store_for_hotkey = Rc::clone(&store);

            // Move `manager` into the closure so Drop (= ungrab) runs when
            // GTK shuts down rather than immediately.
            glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                let _keep = &manager; // keeps manager alive for the closure lifetime
                if rx.try_recv().is_ok() {
                    let entries: Vec<ClipboardEntry> = store_for_hotkey
                        .borrow()
                        .get_all()
                        .into_iter()
                        .cloned()
                        .collect();

                    eprintln!("[hotkey] fired — store has {} item(s)", entries.len());

                    let popup_for_select = Rc::clone(&popup_for_hotkey);
                    popup_for_hotkey.populate(&entries, move |id, content| {
                        println!("Selected item {}", id);
                        if let Some(display) = gdk4::Display::default() {
                            display.clipboard().set_text(&content);
                        }
                        popup_for_select.hide();
                    });

                    popup_for_hotkey.show_at_cursor();
                }
                glib::ControlFlow::Continue
            });
        });

        app.run();
        Ok(())
    }
}
