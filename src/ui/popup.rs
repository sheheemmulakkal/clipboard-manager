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
use crate::config::{AppConfig, ThemeName};
use crate::events::{MenuAction, PopupEvent, RowAction};
use crate::platform::Platform;
use crate::ui::icons::{self, Icon};
use crate::ui::item_row::{build_item_row, RowContext};
use crate::ui::style::generate_css;
use crate::ui::theme::Theme;

/// Space around the card for its drop shadow (only with a compositor).
const SHADOW_MARGIN: i32 = 12;

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
    theme:               Rc<Theme>,
    show_timestamps:     bool,
    search_entry:        SearchEntry,
    suppress_close:      Rc<Cell<u32>>,
    size:                (i32, i32),
    screen_sizes:        Rc<Vec<(u32, u32)>>,
}

impl ClipboardPopup {
    pub fn new(app: &Application, platform: Arc<dyn Platform>, config: &AppConfig) -> Self {
        let display = gdk4::Display::default().expect("no GDK display");
        let composited = display.is_composited();

        let mut theme = Theme::resolve(config.theme, &config.colors);

        let provider = CssProvider::new();
        provider.load_from_data(&generate_css(&theme, &config.sizes));
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        let margin = if composited { SHADOW_MARGIN } else { 0 };
        let size = (config.popup_width + 2 * margin, config.popup_height + 2 * margin);

        let window = Window::builder()
            .application(app)
            .decorated(false)
            .resizable(false)
            .title("Clipboard Manager")
            .default_width(size.0)
            .default_height(size.1)
            .build();
        window.add_css_class("cm-popup");
        if !composited {
            window.add_css_class("no-compositing");
        }

        // SVG icons need a concrete colour; take it from the GTK theme.
        if config.theme == ThemeName::System {
            #[allow(deprecated)] // Widget::color() needs GTK 4.10; 22.04 ships 4.6
            let fg = window.style_context().color();
            theme.adopt_foreground(&rgba_hex(&fg));
        }
        let theme = Rc::new(theme);

        // ── Layout ────────────────────────────────────────────────────────────
        let card = gtk4::Box::new(Orientation::Vertical, 0);
        card.add_css_class("popup-card");
        card.set_overflow(gtk4::Overflow::Hidden);

        // ── Header ────────────────────────────────────────────────────────────
        let handle = WindowHandle::new();
        handle.add_css_class("popup-header");

        let header_row = gtk4::Box::new(Orientation::Horizontal, 4);

        // A CenterBox keeps the icon centred in the fixed-size tile.
        let app_icon = gtk4::CenterBox::new();
        app_icon.add_css_class("app-icon");
        let app_img = icons::image(Icon::FileText, &theme.icon, 20);
        app_img.set_halign(gtk4::Align::Center);
        app_img.set_valign(gtk4::Align::Center);
        app_icon.set_halign(gtk4::Align::Start);
        app_icon.set_valign(gtk4::Align::Center);
        app_icon.set_center_widget(Some(&app_img));

        let title = Label::new(Some("Clipboard Manager"));
        title.add_css_class("popup-title");
        title.set_hexpand(true);
        title.set_halign(gtk4::Align::Start);

        let keep_open = Rc::new(Cell::new(false));
        let pin_btn = icon_button(Icon::Pin, &theme.icon_muted, 18, "header-btn");
        pin_btn.set_tooltip_text(Some("Keep open"));
        {
            let keep_open = Rc::clone(&keep_open);
            let theme = Rc::clone(&theme);
            pin_btn.connect_clicked(move |b| {
                let on = !keep_open.get();
                keep_open.set(on);
                let (icon, color) = if on {
                    (Icon::PinFilled, theme.accent_icon())
                } else {
                    (Icon::Pin, theme.icon_muted.clone())
                };
                b.set_child(Some(&icons::image(icon, &color, 18)));
                b.set_tooltip_text(Some(if on { "Keep open: on" } else { "Keep open" }));
            });
        }

        let suppress_close: Rc<Cell<u32>> = Rc::new(Cell::new(0));
        let handler = EventHandler::default();

        let menu_btn = icon_button(Icon::Menu, &theme.icon_muted, 18, "header-btn");
        menu_btn.set_tooltip_text(Some("Menu"));
        {
            let menu = build_header_menu(&theme, &handler, &suppress_close);
            menu.set_parent(&menu_btn);
            menu_btn.connect_clicked(move |_| menu.popup());
        }

        let close_btn = icon_button(Icon::X, &theme.icon, 16, "close-btn");
        close_btn.set_tooltip_text(Some("Close (Esc)"));

        header_row.append(&app_icon);
        header_row.append(&title);
        header_row.append(&pin_btn);
        header_row.append(&menu_btn);
        header_row.append(&close_btn);
        handle.set_child(Some(&header_row));
        card.append(&handle);

        // ── Search box ────────────────────────────────────────────────────────
        let search_box = gtk4::Box::new(Orientation::Horizontal, 6);
        search_box.add_css_class("search-box");

        let search_entry = SearchEntry::new();
        search_entry.add_css_class("search-entry");
        search_entry.set_placeholder_text(Some("Search clipboard\u{2026}"));
        search_entry.set_hexpand(true);

        let kbd_chip = Label::new(Some("Ctrl + K"));
        kbd_chip.add_css_class("kbd-chip");
        kbd_chip.set_valign(gtk4::Align::Center);

        search_box.append(&search_entry);
        search_box.append(&kbd_chip);
        card.append(&search_box);

        // ── List ──────────────────────────────────────────────────────────────
        let scrolled = ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .vexpand(true)
            .build();

        let list_box = ListBox::new();
        list_box.add_css_class("history");
        list_box.set_selection_mode(SelectionMode::Single);
        scrolled.set_child(Some(&list_box));

        // Scroll-to-top button (floats over the list).
        let list_overlay = gtk4::Overlay::new();
        list_overlay.set_child(Some(&scrolled));
        let scroll_top_btn = icon_button(Icon::ArrowUp, "#ffffff", 18, "scroll-top-btn");
        scroll_top_btn.set_tooltip_text(Some("Scroll to top (Home)"));
        scroll_top_btn.set_halign(gtk4::Align::End);
        scroll_top_btn.set_valign(gtk4::Align::End);
        scroll_top_btn.set_margin_end(16);
        scroll_top_btn.set_margin_bottom(16);
        scroll_top_btn.set_visible(false);
        list_overlay.add_overlay(&scroll_top_btn);
        card.append(&list_overlay);

        {
            let btn = scroll_top_btn.clone();
            let row_height = config.sizes.row_height;
            scrolled.vadjustment().connect_value_changed(move |adj| {
                btn.set_visible(scroll_top_visible(adj.value(), row_height));
            });
        }
        {
            let sw = scrolled.clone();
            let lb = list_box.clone();
            scroll_top_btn.connect_clicked(move |_| {
                animate_scroll_to(&sw, 0.0);
                if let Some(first) = lb.row_at_index(0) {
                    lb.select_row(Some(&first));
                }
            });
        }

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
        card.append(&undo_bar);

        window.set_child(Some(&card));

        // ── Shared state ──────────────────────────────────────────────────────
        let row_ids:      Rc<RefCell<Vec<u64>>>               = Rc::new(RefCell::new(vec![]));
        let undo_pending: Rc<RefCell<Option<UndoPending>>>    = Rc::new(RefCell::new(None));
        let undo_tick:    Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));

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

        // ── Wire: close button ────────────────────────────────────────────────
        {
            let win = window.clone();
            close_btn.connect_clicked(move |_| win.set_visible(false));
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

            key_ctrl.connect_key_pressed(move |_, key, _, mods| {
                use glib::Propagation;
                let ctrl = mods.contains(gdk4::ModifierType::CONTROL_MASK);
                let in_search = has_focus_within(&se);
                match key {
                    k if k == gdk4::Key::Escape => {
                        if !se.text().is_empty() {
                            se.set_text("");
                        } else {
                            win_ref.set_visible(false);
                        }
                        Propagation::Stop
                    }
                    k if (ctrl && (k == gdk4::Key::k || k == gdk4::Key::f))
                        || (k == gdk4::Key::slash && !in_search) =>
                    {
                        se.grab_focus();
                        se.select_region(0, -1);
                        Propagation::Stop
                    }
                    k if (k == gdk4::Key::Home || k == gdk4::Key::End) && !in_search => {
                        let target = if k == gdk4::Key::Home {
                            lb.row_at_index(0)
                        } else {
                            last_row(&lb)
                        };
                        if let Some(row) = target {
                            lb.select_row(Some(&row));
                            row.grab_focus();
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
                        // From the search entry, Down goes to the first row
                        // (which is pre-selected) instead of skipping it.
                        let next = if in_search {
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
            let ko   = Rc::clone(&keep_open);
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

                // A popover (menu, editor) is open, or "keep open" is on.
                if sc.get() > 0 || ko.get() { return; }

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
                    tracing::debug!("[popup] focus lost — closing");
                    do_close(win, &ut, &up, &bar);
                }
            });
        }

        Self {
            window, scrolled, list_box, row_ids, handler,
            undo_bar, undo_label, undo_pending, undo_tick,
            platform, theme, show_timestamps: config.show_timestamps,
            search_entry, suppress_close, size,
            screen_sizes: Rc::new(monitor_sizes(&display)),
        }
    }

    /// Set the single receiver of everything the user does in the popup.
    pub fn set_event_handler(&self, f: Rc<dyn Fn(PopupEvent)>) {
        *self.handler.0.borrow_mut() = Some(f);
    }

    // ── populate ──────────────────────────────────────────────────────────────

    /// Rebuild the list. `empty_text` is shown when `entries` is empty.
    pub fn populate(&self, entries: &[ClipboardEntry], empty_text: &str) {
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

        let ctx = RowContext {
            theme:           Rc::clone(&self.theme),
            show_timestamps: self.show_timestamps,
            suppress_close:  Rc::clone(&self.suppress_close),
            now:             crate::clipboard::entry::now_secs(),
            screen_sizes:    Rc::clone(&self.screen_sizes),
        };
        for entry in entries {
            ids.push(entry.id);
            let id = entry.id;
            let h  = self.handler.clone();
            let row = build_item_row(entry, &ctx, move |action| {
                h.emit(PopupEvent::Row(id, action));
            });
            self.list_box.append(&row);
        }
        drop(ids);

        if entries.is_empty() {
            let row   = gtk4::ListBoxRow::new();
            let label = Label::new(Some(empty_text));
            label.add_css_class("empty-label");
            label.set_margin_top(32);
            label.set_margin_bottom(32);
            row.set_activatable(false);
            row.set_selectable(false);
            row.set_child(Some(&label));
            self.list_box.append(&row);
        } else if is_repopulate {
            // Restore scroll position — keeps the user's view stable after
            // a delete, pin toggle, or label change.
            self.scrolled.vadjustment().set_value(saved_scroll);
        } else {
            // Fresh open: start at the top with row 0 selected so keyboard
            // navigation works immediately.
            self.scrolled.vadjustment().set_value(0.0);
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
        let size     = self.size;

        glib::timeout_add_local_once(Duration::from_millis(50), move || {
            se.grab_focus();
            if let Some((cx, cy)) = cursor {
                move_window_near_cursor(&win, &*platform, size, cx, cy);
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

    pub fn show_about(&self) {
        let about = gtk4::AboutDialog::builder()
            .transient_for(&self.window)
            .modal(true)
            .program_name("Clipboard Manager")
            .version(env!("CARGO_PKG_VERSION"))
            .comments("Clipboard history for Linux")
            .website(env!("CARGO_PKG_HOMEPAGE"))
            .license_type(gtk4::License::MitX11)
            .logo_icon_name("edit-paste")
            .build();
        // The dialog takes focus from the popup; don't treat that as "close".
        self.suppress_close.set(self.suppress_close.get() + 1);
        let sc = Rc::clone(&self.suppress_close);
        about.connect_close_request(move |_| {
            sc.set(sc.get().saturating_sub(1));
            glib::Propagation::Proceed
        });
        about.present();
    }

    pub fn quit(&self) {
        if let Some(app) = self.window.application() {
            app.quit();
        }
    }
}

// ── Header menu ───────────────────────────────────────────────────────────────

fn build_header_menu(theme: &Theme, handler: &EventHandler, suppress: &Rc<Cell<u32>>) -> gtk4::Popover {
    let popover = gtk4::Popover::new();
    popover.add_css_class("cm-menu");
    popover.set_has_arrow(false);
    popover.set_position(gtk4::PositionType::Bottom);

    let vbox = gtk4::Box::new(Orientation::Vertical, 0);
    let items: [(Icon, &str, fn() -> PopupEvent); 4] = [
        (Icon::Trash,     "Clear history", || PopupEvent::ClearAll),
        (Icon::Settings,  "Settings",      || PopupEvent::Menu(MenuAction::OpenSettings)),
        (Icon::Clipboard, "About",         || PopupEvent::Menu(MenuAction::About)),
        (Icon::Power,     "Quit",          || PopupEvent::Menu(MenuAction::Quit)),
    ];
    for (i, (icon, label, event)) in items.into_iter().enumerate() {
        if i == 1 {
            vbox.append(&menu_separator());
        }
        let btn = menu_item(icon, label, None, &theme.icon_muted);
        let h = handler.clone();
        let p = popover.clone();
        btn.connect_clicked(move |_| {
            p.popdown();
            h.emit(event());
        });
        vbox.append(&btn);
    }
    popover.set_child(Some(&vbox));
    track_popover(&popover, suppress);
    popover
}

/// A flat menu row: `[icon] label ……… accel`.
pub fn menu_item(icon: Icon, label: &str, accel: Option<&str>, icon_color: &str) -> Button {
    let row = gtk4::Box::new(Orientation::Horizontal, 10);
    row.append(&icons::image(icon, icon_color, 16));
    let l = Label::new(Some(label));
    l.add_css_class("menu-label");
    l.set_hexpand(true);
    l.set_halign(gtk4::Align::Start);
    row.append(&l);
    if let Some(a) = accel {
        let al = Label::new(Some(a));
        al.add_css_class("menu-accel");
        row.append(&al);
    }
    let btn = Button::new();
    btn.add_css_class("menu-item");
    btn.set_child(Some(&row));
    btn
}

pub fn menu_separator() -> gtk4::Separator {
    let sep = gtk4::Separator::new(Orientation::Horizontal);
    sep.add_css_class("menu-sep");
    sep
}

/// Keep the popup open while `popover` is shown: the popover takes focus,
/// which would otherwise look like the popup losing focus.
pub fn track_popover(popover: &gtk4::Popover, suppress: &Rc<Cell<u32>>) {
    {
        let sc = Rc::clone(suppress);
        popover.connect_show(move |_| sc.set(sc.get() + 1));
    }
    {
        // Decrement one idle tick later so a focus-out that arrives while the
        // popover is closing (or when another popover opens right after)
        // never sees the counter at 0.
        let sc = Rc::clone(suppress);
        popover.connect_closed(move |_| {
            let sc = Rc::clone(&sc);
            glib::idle_add_local_once(move || sc.set(sc.get().saturating_sub(1)));
        });
    }
}

/// Button whose only content is an icon.
fn icon_button(icon: Icon, color: &str, px: i32, class: &str) -> Button {
    let b = Button::new();
    b.add_css_class(class);
    b.set_child(Some(&icons::image(icon, color, px)));
    b.set_valign(gtk4::Align::Center);
    b
}

/// Monitor sizes in device pixels (what a full-screen screenshot measures).
fn monitor_sizes(display: &gdk4::Display) -> Vec<(u32, u32)> {
    let monitors = display.monitors();
    (0..monitors.n_items())
        .filter_map(|i| monitors.item(i).and_downcast::<gdk4::Monitor>())
        .map(|m| {
            let g = m.geometry();
            let s = m.scale_factor().max(1);
            ((g.width() * s) as u32, (g.height() * s) as u32)
        })
        .collect()
}

fn rgba_hex(c: &gdk4::RGBA) -> String {
    let to = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", to(c.red()), to(c.green()), to(c.blue()))
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

/// The scroll-to-top button shows once the list is scrolled past one row.
fn scroll_top_visible(value: f64, row_height: u32) -> bool {
    value > row_height as f64
}

fn ease_out_cubic(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Smoothly scroll `sw` to `target` over 200 ms.
fn animate_scroll_to(sw: &ScrolledWindow, target: f64) {
    const DURATION_US: f64 = 200_000.0;
    let adj   = sw.vadjustment();
    let start = adj.value();
    let t0: Cell<Option<i64>> = Cell::new(None);
    sw.add_tick_callback(move |_, clock| {
        let now = clock.frame_time();
        let t0 = match t0.get() {
            Some(t) => t,
            None => {
                t0.set(Some(now));
                now
            }
        };
        let p = (now - t0) as f64 / DURATION_US;
        adj.set_value(start + (target - start) * ease_out_cubic(p));
        if p >= 1.0 { glib::ControlFlow::Break } else { glib::ControlFlow::Continue }
    });
}

fn last_row(lb: &ListBox) -> Option<gtk4::ListBoxRow> {
    lb.last_child().and_then(|w| w.downcast::<gtk4::ListBoxRow>().ok())
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
fn move_window_near_cursor(win: &Window, platform: &dyn Platform, size: (i32, i32), cx: i32, cy: i32) {
    let (w, h) = size;
    let (sw, sh) = crate::platform::x11::screen_dimensions().unwrap_or((1920, 1080));
    let mut x = cx + 4;
    let mut y = cy + 4;
    if x + w > sw { x = sw - w - 8; }
    if y + h > sh { y = sh - h - 8; }
    if x < 0 { x = 4; }
    if y < 0 { y = 4; }
    platform.move_popup(win, x, y);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ease_out_cubic_endpoints_and_shape() {
        assert_eq!(ease_out_cubic(0.0), 0.0);
        assert_eq!(ease_out_cubic(1.0), 1.0);
        assert!(ease_out_cubic(0.5) > 0.5); // fast start, slow end
        assert_eq!(ease_out_cubic(2.0), 1.0); // clamped
    }

    #[test]
    fn scroll_top_button_appears_after_one_row() {
        assert!(!scroll_top_visible(0.0, 44));
        assert!(!scroll_top_visible(44.0, 44));
        assert!(scroll_top_visible(45.0, 44));
    }

    #[test]
    fn rgba_to_hex() {
        assert_eq!(rgba_hex(&gdk4::RGBA::new(1.0, 0.5, 0.0, 1.0)), "#ff8000");
    }
}
