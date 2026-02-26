use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{
    Application, Button, CssProvider, EventControllerKey, GestureClick, Label, ListBox,
    Orientation, ScrolledWindow, SearchEntry, SelectionMode, Window, WindowHandle,
};

use crate::clipboard::ClipboardEntry;
use crate::config::{ColorConfig, SizeConfig};
use crate::platform::Platform;
use crate::ui::item_row::{RowAction, build_item_row};
use crate::ui::style::generate_css;

// ── Undo state ────────────────────────────────────────────────────────────────

struct UndoPending {
    on_commit: Rc<dyn Fn()>,
    on_undo:   Rc<dyn Fn()>,
}

// ── Public struct ─────────────────────────────────────────────────────────────

pub struct ClipboardPopup {
    window:              Window,
    list_box:            ListBox,
    row_data:            Rc<RefCell<Vec<(u64, String, bool)>>>,
    on_select:           Rc<RefCell<Option<Rc<dyn Fn(u64, String)>>>>,
    on_copy:             Rc<RefCell<Option<Rc<dyn Fn(u64, String)>>>>,
    on_terminal_paste:   Rc<RefCell<Option<Rc<dyn Fn(u64, String)>>>>,
    on_remove:           Rc<RefCell<Option<Rc<dyn Fn(u64)>>>>,
    on_pin:              Rc<RefCell<Option<Rc<dyn Fn(u64, bool)>>>>,
    on_clear:            Rc<RefCell<Option<Rc<dyn Fn()>>>>,
    undo_bar:            gtk4::Box,
    undo_label:          Label,
    undo_pending:        Rc<RefCell<Option<UndoPending>>>,
    undo_tick:           Rc<RefCell<Option<glib::SourceId>>>,
    platform:            Arc<dyn Platform>,
    nerd_font:           bool,
    search_entry:        SearchEntry,
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
        let row_data:           Rc<RefCell<Vec<(u64, String, bool)>>>         = Rc::new(RefCell::new(vec![]));
        let on_select:          Rc<RefCell<Option<Rc<dyn Fn(u64, String)>>>> = Rc::new(RefCell::new(None));
        let on_copy:            Rc<RefCell<Option<Rc<dyn Fn(u64, String)>>>> = Rc::new(RefCell::new(None));
        let on_terminal_paste:  Rc<RefCell<Option<Rc<dyn Fn(u64, String)>>>> = Rc::new(RefCell::new(None));
        let on_remove:          Rc<RefCell<Option<Rc<dyn Fn(u64)>>>>         = Rc::new(RefCell::new(None));
        let on_pin:             Rc<RefCell<Option<Rc<dyn Fn(u64, bool)>>>>   = Rc::new(RefCell::new(None));
        let on_clear:           Rc<RefCell<Option<Rc<dyn Fn()>>>>            = Rc::new(RefCell::new(None));
        let undo_pending:       Rc<RefCell<Option<UndoPending>>>             = Rc::new(RefCell::new(None));
        let undo_tick:          Rc<RefCell<Option<glib::SourceId>>>          = Rc::new(RefCell::new(None));

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
            let oc = Rc::clone(&on_clear);
            clear_btn.connect_clicked(move |_| {
                let cb = oc.borrow().as_ref().map(Rc::clone);
                if let Some(cb) = cb { cb(); }
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
            let rd      = Rc::clone(&row_data);
            let os      = Rc::clone(&on_select);
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
                        let next = if se.has_focus() {
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
                            let idx = row.index() as usize;
                            let (id, content) = {
                                let data = rd.borrow();
                                data.get(idx).map(|(id, c, _)| (*id, c.clone()))
                            }
                            .unwrap_or_default();
                            let cb = os.borrow().as_ref().map(Rc::clone);
                            if let Some(cb) = cb { cb(id, content); }
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
            window, list_box, row_data,
            on_select, on_copy, on_terminal_paste, on_remove, on_pin, on_clear,
            undo_bar, undo_label, undo_pending, undo_tick,
            platform, nerd_font, search_entry,
        }
    }

    // ── populate ──────────────────────────────────────────────────────────────

    pub fn populate(
        &self,
        entries:            &[ClipboardEntry],
        on_select:          impl Fn(u64, String) + 'static,
        on_copy:            impl Fn(u64, String) + 'static,
        on_terminal_paste:  impl Fn(u64, String) + 'static,
        on_remove:          impl Fn(u64)         + 'static,
        on_pin:             impl Fn(u64, bool)   + 'static,
        on_clear:           impl Fn()            + 'static,
    ) {
        cancel_tick(&self.undo_tick);
        *self.undo_pending.borrow_mut() = None;
        self.undo_bar.set_visible(false);

        while let Some(child) = self.list_box.first_child() {
            self.list_box.remove(&child);
        }

        let on_select:         Rc<dyn Fn(u64, String)> = Rc::new(on_select);
        let on_copy:           Rc<dyn Fn(u64, String)> = Rc::new(on_copy);
        let on_terminal_paste: Rc<dyn Fn(u64, String)> = Rc::new(on_terminal_paste);
        let on_remove:         Rc<dyn Fn(u64)>         = Rc::new(on_remove);
        let on_pin:            Rc<dyn Fn(u64, bool)>   = Rc::new(on_pin);
        let on_clear:          Rc<dyn Fn()>            = Rc::new(on_clear);

        *self.on_select.borrow_mut()         = Some(Rc::clone(&on_select));
        *self.on_copy.borrow_mut()           = Some(Rc::clone(&on_copy));
        *self.on_terminal_paste.borrow_mut() = Some(Rc::clone(&on_terminal_paste));
        *self.on_remove.borrow_mut()         = Some(Rc::clone(&on_remove));
        *self.on_pin.borrow_mut()            = Some(Rc::clone(&on_pin));
        *self.on_clear.borrow_mut()          = Some(Rc::clone(&on_clear));

        let mut data = self.row_data.borrow_mut();
        data.clear();

        for entry in entries {
            data.push((entry.id, entry.content.clone(), entry.pinned));

            let id      = entry.id;
            let content = entry.content.clone();
            let cb_sel  = Rc::clone(&on_select);
            let cb_cpy  = Rc::clone(&on_copy);
            let cb_tp   = Rc::clone(&on_terminal_paste);
            let cb_rm   = Rc::clone(&on_remove);
            let cb_pin  = Rc::clone(&on_pin);

            let row = build_item_row(entry, self.nerd_font, move |action| match action {
                RowAction::Select            => cb_sel(id, content.clone()),
                RowAction::Copy              => cb_cpy(id, content.clone()),
                RowAction::TerminalPaste     => cb_tp(id, content.clone()),
                RowAction::Remove            => cb_rm(id),
                RowAction::TogglePin(pinned) => cb_pin(id, pinned),
            });
            self.list_box.append(&row);
        }
        drop(data);

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
        } else if let Some(first) = self.list_box.row_at_index(0) {
            self.list_box.select_row(Some(&first));
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
        let mut idx = 0i32;
        loop {
            match self.list_box.row_at_index(idx) {
                None => break,
                Some(row) => {
                    let pinned = self.row_data
                        .borrow()
                        .get(idx as usize)
                        .map(|(_, _, p)| *p)
                        .unwrap_or(false);
                    if pinned { idx += 1; } else { self.list_box.remove(&row); }
                }
            }
        }
        self.row_data.borrow_mut().retain(|(_, _, pinned)| *pinned);

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

    pub fn connect_search_changed(&self, cb: impl Fn(String) + 'static) {
        self.search_entry.connect_search_changed(move |se| {
            cb(se.text().to_string());
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
