//! The controller owns the store, the popup and the platform backend, and
//! turns [`AppEvent`]s and [`PopupEvent`]s into store mutations and UI
//! updates. Everything here runs on the GTK main thread.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gdk4::prelude::*;

use crate::cli::Command;
use crate::clipboard::entry::{ClipboardContent, ClipboardEntry};
use crate::config::AppConfig;
use crate::events::{AppEvent, MenuAction, PopupEvent, RowAction};
use crate::platform::Platform;
use crate::store::Store;
use crate::ui::filter::{self, Chip};
use crate::ui::ClipboardPopup;

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
    chip:        RefCell<Chip>,
    /// Shared with the clipboard monitor, which skips changes while set.
    paused:      Rc<Cell<bool>>,
    pause_listeners: RefCell<Vec<PauseListener>>,
}

type PauseListener = Box<dyn Fn(bool)>;

/// New pause state for a request (`None` = toggle).
fn resolve_paused(current: bool, request: Option<bool>) -> bool {
    request.unwrap_or(!current)
}

impl Controller {
    pub fn new(
        config:   AppConfig,
        store:    Rc<RefCell<Box<dyn Store>>>,
        popup:    ClipboardPopup,
        platform: Arc<dyn Platform>,
        paused:   Rc<Cell<bool>>,
    ) -> Rc<Self> {
        let this = Rc::new(Self {
            config,
            store,
            popup,
            platform,
            prev_window: Cell::new(None),
            query:       RefCell::new(String::new()),
            chip:        RefCell::new(Chip::All),
            paused,
            pause_listeners: RefCell::new(Vec::new()),
        });
        let weak = Rc::downgrade(&this);
        this.popup.set_event_handler(Rc::new(move |ev| {
            if let Some(c) = weak.upgrade() {
                crate::crash::guarded("popup event", || c.handle_popup(ev));
            }
        }));
        this
    }

    pub fn handle_app(&self, ev: AppEvent) {
        match ev {
            AppEvent::Show { prev_window } => self.show(prev_window),
            AppEvent::Toggle { prev_window } => {
                if self.popup.is_visible() {
                    self.popup.hide();
                } else {
                    self.show(prev_window);
                }
            }
            AppEvent::SetPaused(request) => self.set_paused(request),
            AppEvent::OpenSettings => self.handle_menu(MenuAction::OpenSettings),
            AppEvent::Quit => self.handle_menu(MenuAction::Quit),
        }
    }

    /// The monitor recorded something: show it if the popup is open (e.g.
    /// with "keep open" on), unless a popover is open over the list.
    pub fn on_store_changed(&self) {
        if self.popup.is_visible() && !self.popup.is_busy() {
            self.refresh();
        }
    }

    /// Execute a command forwarded from another `clipboard-manager` process.
    /// Returns whether it succeeded.
    pub fn run_command(&self, cmd: Command, activation_token: Option<String>) -> bool {
        if let Some(t) = &activation_token {
            self.popup.set_startup_id(t);
        }
        let prev = || self.platform.capture_active_window();
        match cmd {
            Command::Start | Command::Show => self.handle_app(AppEvent::Show { prev_window: prev() }),
            Command::Toggle => self.handle_app(AppEvent::Toggle { prev_window: prev() }),
            Command::Pause => self.set_paused(Some(true)),
            Command::Resume => self.set_paused(Some(false)),
            Command::TogglePause => self.set_paused(None),
            Command::Clear => {
                self.store.borrow_mut().clear_unpinned();
                if self.popup.is_visible() {
                    self.refresh();
                }
            }
            Command::Quit => self.popup.quit(),
            // Handled by the client process itself.
            Command::List { .. } | Command::Reload | Command::Help | Command::Version => return false,
        }
        true
    }

    /// Apply `expire_after_days` now and then hourly.
    pub fn start_expiry(self: &Rc<Self>) {
        let days = self.config.expire_after_days;
        if days == 0 {
            return;
        }
        let expire = {
            let weak = Rc::downgrade(self);
            move || {
                let Some(c) = weak.upgrade() else { return glib::ControlFlow::Break };
                let cutoff = crate::clipboard::entry::now_secs().saturating_sub(days * 86_400);
                let n = c.store.borrow_mut().expire_older_than(cutoff);
                if n > 0 {
                    tracing::info!("[expire] removed {n} item(s) older than {days} day(s)");
                    if c.popup.is_visible() {
                        c.refresh();
                    }
                }
                glib::ControlFlow::Continue
            }
        };
        expire();
        glib::timeout_add_seconds_local(3600, expire);
    }

    /// Call `f` with the new state whenever capture is paused or resumed.
    pub fn on_pause_changed(&self, f: impl Fn(bool) + 'static) {
        self.pause_listeners.borrow_mut().push(Box::new(f));
    }

    fn set_paused(&self, request: Option<bool>) {
        let paused = resolve_paused(self.paused.get(), request);
        self.paused.set(paused);
        tracing::info!("[capture] {}", if paused { "paused" } else { "resumed" });
        self.popup.set_paused(paused);
        for f in self.pause_listeners.borrow().iter() {
            f(paused);
        }
    }

    pub fn handle_popup(self: &Rc<Self>, ev: PopupEvent) {
        match ev {
            PopupEvent::Row(id, action) => self.handle_row(id, action),
            PopupEvent::SearchChanged(q) => {
                *self.query.borrow_mut() = q;
                self.refresh();
            }
            PopupEvent::ChipChanged(chip) => {
                *self.chip.borrow_mut() = chip;
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
            MenuAction::TogglePause => self.set_paused(None),
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
        let chip = self.chip.borrow().clone();
        // A tag chip whose tag no longer exists falls back to "All".
        let chip = match chip {
            Chip::Tag(ref t) if !tags.iter().any(|x| x.eq_ignore_ascii_case(t)) => Chip::All,
            c => c,
        };
        let entries = filter::visible(all, &query, &chip);
        self.popup.populate(&entries, empty_text, tags, &chip);
    }

    fn show(&self, prev_window: Option<u64>) {
        self.prev_window.set(prev_window);
        self.query.borrow_mut().clear();
        *self.chip.borrow_mut() = Chip::All;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_request_resolution() {
        assert!(resolve_paused(false, None));
        assert!(!resolve_paused(true, None));
        assert!(resolve_paused(true, Some(true)));
        assert!(!resolve_paused(true, Some(false)));
    }
}
