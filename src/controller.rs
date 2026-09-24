//! The controller owns the store, the popup and the platform backend, and
//! turns [`AppEvent`]s and [`PopupEvent`]s into store mutations and UI
//! updates. Everything here runs on the GTK main thread.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gdk4::prelude::*;

use crate::clipboard::entry::{ClipboardContent, ClipboardEntry};
use crate::config::AppConfig;
use crate::events::{AppEvent, MenuAction, PopupEvent, RowAction};
use crate::platform::Platform;
use crate::store::Store;
use crate::ui::{filter, ClipboardPopup};

/// Delay between hiding the popup and sending the paste keystroke, so the
/// previous window has regained focus.
const PASTE_DELAY: Duration = Duration::from_millis(200);

pub struct Controller {
    config:      AppConfig,
    store:       Rc<RefCell<Box<dyn Store>>>,
    popup:       ClipboardPopup,
    platform:    Arc<dyn Platform>,
    prev_window: Cell<Option<u64>>,
    query:       RefCell<String>,
}

impl Controller {
    pub fn new(
        config:   AppConfig,
        store:    Rc<RefCell<Box<dyn Store>>>,
        popup:    ClipboardPopup,
        platform: Arc<dyn Platform>,
    ) -> Rc<Self> {
        let this = Rc::new(Self {
            config,
            store,
            popup,
            platform,
            prev_window: Cell::new(None),
            query:       RefCell::new(String::new()),
        });
        let weak = Rc::downgrade(&this);
        this.popup.set_event_handler(Rc::new(move |ev| {
            if let Some(c) = weak.upgrade() {
                c.handle_popup(ev);
            }
        }));
        this
    }

    pub fn handle_app(&self, ev: AppEvent) {
        match ev {
            AppEvent::Show { prev_window } => self.show(prev_window),
        }
    }

    pub fn handle_popup(self: &Rc<Self>, ev: PopupEvent) {
        match ev {
            PopupEvent::Row(id, action) => self.handle_row(id, action),
            PopupEvent::SearchChanged(q) => {
                *self.query.borrow_mut() = q;
                self.refresh();
            }
            PopupEvent::ClearAll => self.clear_all(),
            PopupEvent::Menu(action) => self.handle_menu(action),
        }
    }

    fn handle_menu(&self, action: MenuAction) {
        match action {
            MenuAction::OpenSettings => {
                self.popup.hide();
                open_settings();
            }
            MenuAction::About => self.popup.show_about(),
            MenuAction::Quit => self.popup.quit(),
        }
    }

    /// Rebuild the popup list from the store and the current search query.
    pub fn refresh(&self) {
        let all: Vec<ClipboardEntry> = self.store.borrow().get_all().into_iter().cloned().collect();
        let query = self.query.borrow();
        let empty_text = if all.is_empty() {
            "Nothing copied yet"
        } else {
            "No matches"
        };
        let mut tags: Vec<String> = all.iter().filter_map(|e| e.tag.clone()).collect();
        tags.sort_by_key(|t| t.to_lowercase());
        tags.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        let entries = filter::visible(all, &query);
        self.popup.populate(&entries, empty_text, tags);
    }

    fn show(&self, prev_window: Option<u64>) {
        self.prev_window.set(prev_window);
        self.query.borrow_mut().clear();
        self.popup.clear_search();
        self.refresh();
        if self.config.popup_follow_cursor {
            self.popup.show_at_cursor();
        } else {
            self.popup.show_centered();
        }
    }

    fn entry(&self, id: u64) -> Option<ClipboardEntry> {
        self.store.borrow().get(id).cloned()
    }

    /// The user re-used an entry: put it on the clipboard and move it to the
    /// top. (The monitor ignores our own clipboard changes.)
    fn use_entry(&self, entry: &ClipboardEntry) {
        set_clipboard_content(&entry.content);
        self.store.borrow_mut().touch(entry.id);
    }

    fn handle_row(&self, id: u64, action: RowAction) {
        tracing::debug!("[row] id={id} {action:?}");
        match action {
            RowAction::Paste => {
                let Some(entry) = self.entry(id) else { return };
                self.use_entry(&entry);
                self.hide_and_paste(false);
            }
            RowAction::PasteTerminal => {
                let Some(entry) = self.entry(id) else { return };
                if !matches!(entry.content, ClipboardContent::Text(_)) {
                    return;
                }
                self.use_entry(&entry);
                self.hide_and_paste(true);
            }
            RowAction::Copy => {
                if let Some(entry) = self.entry(id) {
                    self.use_entry(&entry);
                    self.refresh();
                }
            }
            RowAction::Remove => {
                self.store.borrow_mut().remove(id);
                self.refresh();
            }
            RowAction::TogglePin => {
                let Some(entry) = self.entry(id) else { return };
                self.store.borrow_mut().set_pinned(id, !entry.pinned);
                self.refresh();
            }
            RowAction::SetMeta(meta) => {
                self.store.borrow_mut().set_meta(id, meta);
                self.refresh();
            }
            RowAction::EditContent(text) => {
                self.store.borrow_mut().set_text(id, text);
                self.refresh();
            }
        }
    }

    fn hide_and_paste(&self, terminal: bool) {
        self.popup.hide();
        let prev = self.prev_window.get();
        let platform = Arc::clone(&self.platform);
        glib::timeout_add_local_once(PASTE_DELAY, move || {
            // Paste backends block (X11 round-trips, portal calls).
            std::thread::spawn(move || {
                if terminal {
                    platform.paste_terminal(prev);
                } else {
                    platform.paste(prev);
                }
            });
        });
    }

    /// Remove all unpinned entries now; the undo bar can put them back.
    fn clear_all(self: &Rc<Self>) {
        let snapshot: Vec<ClipboardEntry> = self
            .store
            .borrow()
            .get_all()
            .into_iter()
            .filter(|e| !e.pinned)
            .cloned()
            .collect();
        if snapshot.is_empty() {
            return;
        }
        let count = snapshot.len();
        self.store.borrow_mut().clear_unpinned();
        self.refresh();
        tracing::debug!("[clear] cleared {count} item(s)");

        let timeout = self.config.clear_undo_timeout_secs;
        if timeout == 0 {
            return;
        }
        let snapshot = RefCell::new(Some(snapshot));
        let weak = Rc::downgrade(self);
        self.popup.show_undo_bar(
            count,
            timeout,
            move || {
                let (Some(c), Some(entries)) = (weak.upgrade(), snapshot.borrow_mut().take()) else {
                    return;
                };
                c.store.borrow_mut().restore(entries);
                c.refresh();
                tracing::debug!("[clear] undone");
            },
            || tracing::debug!("[clear] committed"),
        );
    }
}

/// Open config.toml in the user's default editor (creating it first).
fn open_settings() {
    let path = crate::paths::config_file();
    if !path.exists() {
        crate::config::AppConfig::write_default(&path);
    }
    let uri = gdk4::gio::File::for_path(&path).uri();
    if let Err(e) = gdk4::gio::AppInfo::launch_default_for_uri(&uri, None::<&gdk4::gio::AppLaunchContext>) {
        crate::notify::error("Cannot open settings", &format!("{}: {e}", path.display()));
    }
}

/// Put text or an image (loaded from disk) on the clipboard.
fn set_clipboard_content(content: &ClipboardContent) {
    let Some(display) = gdk4::Display::default() else { return };
    match content {
        ClipboardContent::Text(t) => display.clipboard().set_text(t),
        ClipboardContent::Image { hash, .. } => {
            let file = gdk4::gio::File::for_path(crate::paths::image_path(hash));
            match gdk4::Texture::from_file(&file) {
                Ok(texture) => display.clipboard().set_texture(&texture),
                Err(e) => tracing::warn!("[clipboard] failed to load image {}: {e}", crate::paths::hex(hash)),
            }
        }
    }
}
