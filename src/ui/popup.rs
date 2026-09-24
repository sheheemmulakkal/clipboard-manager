use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{
    Application, Button, CssProvider, EventControllerKey, GestureClick, Label, ListBox,
    Orientation, ScrolledWindow, SearchEntry, SelectionMode, Window, WindowHandle,
};

use crate::clipboard::entry::ClipboardEntry;
use crate::config::{ColorConfig, SizeConfig};
use crate::events::{PopupEvent, RowAction};
use crate::platform::Platform;
use crate::ui::item_row::build_item_row;
use crate::ui::style::generate_css;

// ── Undo state ────────────────────────────────────────────────────────────────

struct UndoPending {
    on_commit: Rc<dyn Fn()>,
    on_undo:   Rc<dyn Fn()>,
}

// ── Event handler ─────────────────────────────────────────────────────────────

/// Shared slot for the controller's event callback. Cloned into every
/// signal handler; looked up at emit time so it can be set after the
/// widgets are built.
#[derive(Clone, Default)]
struct EventHandler(Rc<RefCell<Option<Rc<dyn Fn(PopupEvent)>>>>);

impl EventHandler {
    fn emit(&self, ev: PopupEvent) {
        // Clone the callback out first so it may re-enter the popup freely.
        let cb = self.0.borrow().as_ref().map(Rc::clone);
        if let Some(cb) = cb {
            cb(ev);
        }
    }
}

// ── Public struct ─────────────────────────────────────────────────────────────

pub struct ClipboardPopup {
    window:              Window,
    scrolled:            ScrolledWindow,
    list_box:            ListBox,
    /// Entry id of each list row, by row index.
    row_ids:             Rc<RefCell<Vec<u64>>>,
    handler:             EventHandler,
    undo_bar:            gtk4::Box,
    undo_label:          Label,
    undo_pending:        Rc<RefCell<Option<UndoPending>>>,
    undo_tick:           Rc<RefCell<Option<glib::SourceId>>>,
    platform:            Arc<dyn Platform>,
    nerd_font:           bool,
    search_entry:        SearchEntry,
    suppress_close:      Rc<Cell<u32>>,
}

impl ClipboardPopup {
    pub fn new(
        app:       &Application,
        platform:  Arc<dyn Platform>,
        nerd_font: bool,
        colors:    &ColorConfig,
        sizes:     &SizeConfig,
    ) -> Self {
        let provider = CssProvider::new();
        provider.load_from_data(&generate_css(colors, sizes));
        gtk4::style_context_add_provider_for_display(
            &gdk4::Display::default().expect("no GDK display"),
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        let window = Window::builder()
            .application(app)
            .decorated(false)
            .resizable(false)
            .title("Clipboard Manager")
            .default_width(460)
            .default_height(520)
            .build();

        // ── Layout ────────────────────────────────────────────────────────────
        let vbox = gtk4::Box::new(Orientation::Vertical, 0);

        let handle = WindowHandle::new();
        handle.add_css_class("popup-header");

        let header_row = gtk4::Box::new(Orientation::Horizontal, 0);
        let title = Label::new(Some("Clipboard Manager"));
        title.add_css_class("popup-title");
        title.set_hexpand(true);
        title.set_halign(gtk4::Align::Start);

        let clear_btn = Button::with_label("Clear All");
        clear_btn.add_css_class("clear-btn");
        clear_btn.set_valign(gtk4::Align::Center);
        clear_btn.set_tooltip_text(Some("Remove all non-pinned items"));

        header_row.append(&title);
        header_row.append(&clear_btn);
        handle.set_child(Some(&header_row));
        vbox.append(&handle);

        // ── Search bar ────────────────────────────────────────────────────────
        let search_bar = gtk4::Box::new(Orientation::Horizontal, 6);
        search_bar.add_css_class("search-bar");
        search_bar.set_margin_start(10);
        search_bar.set_margin_end(10);
        search_bar.set_margin_top(6);
        search_bar.set_margin_bottom(6);

        let search_entry = SearchEntry::new();
        search_entry.add_css_class("search-entry");
        search_entry.set_placeholder_text(Some("Search\u{2026}"));
        search_entry.set_hexpand(true);

        let esc_hint = Label::new(Some("Esc to clear"));
        esc_hint.add_css_class("search-hint");
        esc_hint.set_visible(false);

        {
            let hint = esc_hint.clone();
            search_entry.connect_changed(move |se| {
                hint.set_visible(!se.text().is_empty());
            });
        }

        search_bar.append(&search_entry);
        search_bar.append(&esc_hint);
        vbox.append(&search_bar);

        let scrolled = ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .vexpand(true)
            .build();

        let list_box = ListBox::new();
        list_box.set_selection_mode(SelectionMode::Single);
        scrolled.set_child(Some(&list_box));
        vbox.append(&scrolled);

        // ── Undo bar ──────────────────────────────────────────────────────────
        let undo_bar   = gtk4::Box::new(Orientation::Horizontal, 8);
        undo_bar.add_css_class("undo-bar");
        let undo_label = Label::new(None);
        undo_label.add_css_class("undo-label");
        undo_label.set_hexpand(true);
        undo_label.set_halign(gtk4::Align::Start);
        let undo_btn = Button::with_label("Undo");
        undo_btn.add_css_class("undo-btn");
        undo_btn.set_valign(gtk4::Align::Center);
        undo_bar.append(&undo_label);
        undo_bar.append(&undo_btn);
        undo_bar.set_visible(false);
        vbox.append(&undo_bar);

        window.set_child(Some(&vbox));

        // ── Shared state ──────────────────────────────────────────────────────
        let row_ids:            Rc<RefCell<Vec<u64>>>                        = Rc::new(RefCell::new(vec![]));
        let handler:            EventHandler                                 = EventHandler::default();
        let undo_pending:       Rc<RefCell<Option<UndoPending>>>             = Rc::new(RefCell::new(None));
        let undo_tick:          Rc<RefCell<Option<glib::SourceId>>>          = Rc::new(RefCell::new(None));
        let suppress_close:     Rc<Cell<u32>>                                = Rc::new(Cell::new(0));

        // ── Drag tracking ─────────────────────────────────────────────────────
        //
        // When the user drags the header, WindowHandle calls begin_move_drag()
        // which hands the pointer grab to the WM.  GTK cancels its own gesture
        // (no connect_released fires).  On some compositors this briefly
        // de-activates the window, falsely triggering "close on focus loss".
        //
        // Strategy:
        //   • A capture-phase GestureClick on the whole window sets `drag_held`
        //     on button-1 press and clears it on button-1 release.
        //   • For a WindowHandle drag, GTK cancels our gesture → released never
        //     fires → drag_held stays true.
        //   • is_active_notify checks drag_held:
        //       - false → genuine app-switch → close immediately
        //       - true  → possible drag → start a 50 ms poll
        //   • The poll uses platform.button1_held() to detect physical release
        //     (works on X11 via x11rb query_pointer; returns false on Wayland).
        let drag_held: Rc<Cell<bool>> = Rc::new(Cell::new(false));

        {
            let gc = GestureClick::new();
            gc.set_button(1);
            gc.set_propagation_phase(gtk4::PropagationPhase::Capture);

            let dh = Rc::clone(&drag_held);
            gc.connect_pressed(move |_, _, _, _| { dh.set(true); });

            let dh = Rc::clone(&drag_held);
            gc.connect_released(move |_, _, _, _| { dh.set(false); });

            window.add_controller(gc);
        }

        // ── Wire: Clear All ───────────────────────────────────────────────────
        {
            let h = handler.clone();
            clear_btn.connect_clicked(move |_| h.emit(PopupEvent::ClearAll));
        }

        // ── Wire: search ──────────────────────────────────────────────────────
        {
            let h = handler.clone();
            search_entry.connect_search_changed(move |se| {
                h.emit(PopupEvent::SearchChanged(se.text().to_string()));
            });
        }

        // ── Wire: Undo button ─────────────────────────────────────────────────
        {
            let up  = Rc::clone(&undo_pending);
            let ut  = Rc::clone(&undo_tick);
            let bar = undo_bar.clone();
            undo_btn.connect_clicked(move |_| {
                cancel_tick(&ut);
                let state = up.borrow_mut().take();
                bar.set_visible(false);
                if let Some(s) = state { (s.on_undo)(); }
            });
        }

        // ── Keyboard handler ──────────────────────────────────────────────────
        {
            let key_ctrl = EventControllerKey::new();
            key_ctrl.set_propagation_phase(gtk4::PropagationPhase::Capture);

            let win_ref = window.clone();
            let lb      = list_box.clone();
            let ids     = Rc::clone(&row_ids);
            let h       = handler.clone();
            let se      = search_entry.clone();

            key_ctrl.connect_key_pressed(move |_, key, _, _| {
                use glib::Propagation;
                match key {
                    k if k == gdk4::Key::Escape => {
                        if !se.text().is_empty() {
                            se.set_text("");
                        } else {
                            win_ref.set_visible(false);
                        }
                        Propagation::Stop
                    }
                    k if k == gdk4::Key::Up => {
                        let idx  = lb.selected_row().map(|r| r.index()).unwrap_or(0);
                        let prev = if idx > 0 { idx - 1 } else { 0 };
                        if let Some(row) = lb.row_at_index(prev) {
                            lb.select_row(Some(&row));
                            row.grab_focus();
                        }
                        Propagation::Stop
                    }
                    k if k == gdk4::Key::Down => {
                        // When search entry has focus, Down always jumps to the
                        // first list item (row 0) rather than advancing from the
                        // currently selected row, which would skip row 0.
                        let next = if has_focus_within(&se) {
                            0
                        } else {
                            lb.selected_row().map(|r| r.index() + 1).unwrap_or(0)
                        };
                        if let Some(row) = lb.row_at_index(next) {
                            lb.select_row(Some(&row));
                            row.grab_focus();
                        }
                        Propagation::Stop
                    }
                    k if k == gdk4::Key::Return || k == gdk4::Key::KP_Enter => {
                        if let Some(row) = lb.selected_row() {
                            let id = ids.borrow().get(row.index() as usize).copied();
                            if let Some(id) = id {
                                h.emit(PopupEvent::Row(id, RowAction::Paste));
                            }
                        }
                        Propagation::Stop
                    }
                    _ => Propagation::Proceed,
                }
            });
            window.add_controller(key_ctrl);
        }

        // ── Focus-loss handler — drag-aware ───────────────────────────────────
        {
            let up   = Rc::clone(&undo_pending);
            let ut   = Rc::clone(&undo_tick);
            let bar  = undo_bar.clone();
            let dh   = Rc::clone(&drag_held);
            let sc   = Rc::clone(&suppress_close);
            let poll: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
            let poll_outer = Rc::clone(&poll);
            let platform_dh = Arc::clone(&platform);

            window.connect_is_active_notify(move |win| {
                if win.is_active() {
                    if let Some(id) = poll_outer.borrow_mut().take() {
                        id.remove();
                    }
                    return;
                }

                // A child popover (label editor) is open — don't close the popup.
                if sc.get() > 0 { return; }

                if dh.get() {
                    let win_c    = win.clone();
                    let dh_c     = Rc::clone(&dh);
                    let up_c     = Rc::clone(&up);
                    let ut_c     = Rc::clone(&ut);
                    let bar_c    = bar.clone();
                    let poll_c   = Rc::clone(&poll_outer);
                    let plat_c   = Arc::clone(&platform_dh);
                    let grace    = Rc::new(Cell::new(0u8));

                    let id = glib::timeout_add_local(Duration::from_millis(50), move || {
                        // Window re-gained focus → drag completed, cancel close.
                        if win_c.is_active() {
                            dh_c.set(false);
                            *poll_c.borrow_mut() = None;
                            return glib::ControlFlow::Break;
                        }

                        if dh_c.get() {
                            if plat_c.can_query_button1() {
                                // X11: if button is no longer held, the drag ended.
                                // Start the short grace countdown.
                                if !plat_c.button1_held() {
                                    dh_c.set(false);
                                    grace.set(0);
                                }
                            } else {
                                // Wayland: can't query button state. Use grace as a
                                // max-wait counter — close only after 5 s with no
                                // re-activation (the normal drag re-activates the
                                // window long before that).
                                let g = grace.get() + 1;
                                grace.set(g);
                                if g >= 100 {
                                    do_close(&win_c, &ut_c, &up_c, &bar_c);
                                    *poll_c.borrow_mut() = None;
                                    return glib::ControlFlow::Break;
                                }
                            }
                        }

                        // X11 only: brief grace after button release before closing.
                        if !dh_c.get() && plat_c.can_query_button1() {
                            let g = grace.get() + 1;
                            grace.set(g);
                            if g >= 4 {
                                do_close(&win_c, &ut_c, &up_c, &bar_c);
                                *poll_c.borrow_mut() = None;
                                return glib::ControlFlow::Break;
                            }
                        }

                        glib::ControlFlow::Continue
                    });

                    *poll_outer.borrow_mut() = Some(id);
                } else {
                    do_close(win, &ut, &up, &bar);
                }
            });
        }

        Self {
            window, scrolled, list_box, row_ids, handler,
            undo_bar, undo_label, undo_pending, undo_tick,
            platform, nerd_font, search_entry, suppress_close,
        }
    }

    /// Set the single receiver of everything the user does in the popup.
    pub fn set_event_handler(&self, f: Rc<dyn Fn(PopupEvent)>) {
        *self.handler.0.borrow_mut() = Some(f);
    }

    // ── populate ──────────────────────────────────────────────────────────────

    pub fn populate(&self, entries: &[ClipboardEntry]) {
        // If the popup is already visible this is a mutation repopulate (delete/pin/label).
        // Save the scroll position so we can restore it after rebuilding the list.
        let is_repopulate = self.window.is_visible();
        let saved_scroll = if is_repopulate {
            self.scrolled.vadjustment().value()
        } else {
            0.0
        };

        while let Some(child) = self.list_box.first_child() {
            self.list_box.remove(&child);
        }

        let mut ids = self.row_ids.borrow_mut();
        ids.clear();

        for entry in entries {
            ids.push(entry.id);
            let id = entry.id;
            let h  = self.handler.clone();
            let row = build_item_row(entry, self.nerd_font, Rc::clone(&self.suppress_close), move |action| {
                h.emit(PopupEvent::Row(id, action));
            });
            self.list_box.append(&row);
        }
        drop(ids);

        if entries.is_empty() {
            let row   = gtk4::ListBoxRow::new();
            let label = Label::new(Some("No matches"));
            label.add_css_class("empty-label");
            label.set_margin_top(16);
            label.set_margin_bottom(16);
            row.set_activatable(false);
            row.set_selectable(false);
            row.set_child(Some(&label));
            self.list_box.append(&row);
        } else if is_repopulate {
            // Restore scroll position — keeps the user's view stable after
            // a delete, pin toggle, or label change.
            self.scrolled.vadjustment().set_value(saved_scroll);
        } else {
            // Fresh open: select row 0 so keyboard navigation works immediately.
            if let Some(first) = self.list_box.row_at_index(0) {
                self.list_box.select_row(Some(&first));
            }
        }
    }

    // ── show_undo_bar ─────────────────────────────────────────────────────────

    pub fn show_undo_bar(
        &self,
        count:        usize,
        timeout_secs: u64,
        on_undo:      impl Fn() + 'static,
        on_commit:    impl Fn() + 'static,
    ) {
        // A previous clear's undo window ends when a new one starts.
        cancel_tick(&self.undo_tick);
        if let Some(prev) = self.undo_pending.borrow_mut().take() {
            (prev.on_commit)();
        }

        let noun = if count == 1 { "item" } else { "items" };
        self.undo_label.set_text(&format!("{count} {noun} cleared  ·  Undo ({timeout_secs}s)"));
        self.undo_bar.set_visible(true);

        let on_commit: Rc<dyn Fn()> = Rc::new(on_commit);
        let on_undo:   Rc<dyn Fn()> = Rc::new(on_undo);

        *self.undo_pending.borrow_mut() = Some(UndoPending {
            on_commit: Rc::clone(&on_commit),
            on_undo:   Rc::clone(&on_undo),
        });

        let remaining = Rc::new(Cell::new(timeout_secs));
        let up  = Rc::clone(&self.undo_pending);
        let ut  = Rc::clone(&self.undo_tick);
        let bar = self.undo_bar.clone();
        let lbl = self.undo_label.clone();
        let noun_s = noun.to_string();

        let tick_id = glib::timeout_add_local(Duration::from_secs(1), move || {
            let rem = remaining.get().saturating_sub(1);
            remaining.set(rem);
            if rem == 0 {
                bar.set_visible(false);
                let state = up.borrow_mut().take();
                *ut.borrow_mut() = None;
                if let Some(s) = state { (s.on_commit)(); }
                glib::ControlFlow::Break
            } else {
                lbl.set_text(&format!("{count} {noun_s} cleared  ·  Undo ({rem}s)"));
                glib::ControlFlow::Continue
            }
        });

        *self.undo_tick.borrow_mut() = Some(tick_id);
    }

    // ── show / hide ───────────────────────────────────────────────────────────

    pub fn show_at_cursor(&self) {
        // Capture cursor position now (before window grabs focus).
        // Returns None on Wayland; the compositor will place the window.
        let cursor = self.platform.cursor_position();

        self.window.present();

        let se       = self.search_entry.clone();
        let win      = self.window.clone();
        let platform = Arc::clone(&self.platform);

        glib::timeout_add_local_once(Duration::from_millis(50), move || {
            se.grab_focus();
            if let Some((cx, cy)) = cursor {
                move_window_near_cursor(&win, &*platform, cx, cy);
            }
        });
    }

    pub fn show_centered(&self) {
        self.window.present();
        let se = self.search_entry.clone();
        glib::timeout_add_local_once(Duration::from_millis(0), move || {
            se.grab_focus();
        });
    }

    pub fn clear_search(&self) {
        self.search_entry.set_text("");
    }

    pub fn hide(&self) {
        self.window.set_visible(false);
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn do_close(
    win: &Window,
    ut:  &Rc<RefCell<Option<glib::SourceId>>>,
    up:  &Rc<RefCell<Option<UndoPending>>>,
    bar: &gtk4::Box,
) {
    win.set_visible(false);
    cancel_tick(ut);
    let state = up.borrow_mut().take();
    bar.set_visible(false);
    if let Some(s) = state { (s.on_commit)(); }
}

/// True when keyboard focus is on `widget` or one of its descendants.
/// (A SearchEntry never has focus itself — its inner text widget does.)
fn has_focus_within(widget: &impl IsA<gtk4::Widget>) -> bool {
    let widget = widget.as_ref();
    widget
        .root()
        .and_then(|r| r.focus())
        .is_some_and(|f| &f == widget || f.is_ancestor(widget))
}

fn cancel_tick(ut: &Rc<RefCell<Option<glib::SourceId>>>) {
    if let Some(id) = ut.borrow_mut().take() { id.remove(); }
}

/// Clamp and move the popup near the cursor, using the platform backend.
fn move_window_near_cursor(win: &Window, platform: &dyn Platform, cx: i32, cy: i32) {
    let w: i32 = 460;
    let h: i32 = 520;
    let (sw, sh) = crate::platform::x11::screen_dimensions().unwrap_or((1920, 1080));
    let mut x = cx + 4;
    let mut y = cy + 4;
    if x + w > sw { x = sw - w - 8; }
    if y + h > sh { y = sh - h - 8; }
    if x < 0 { x = 4; }
    if y < 0 { y = 4; }
    platform.move_popup(win, x, y);
}
