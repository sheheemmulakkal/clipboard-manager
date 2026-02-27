use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use gtk4::prelude::*;
use gtk4::glib;
use gtk4::{Button, Entry, GestureClick, Label, ListBoxRow, Orientation, Popover};

use crate::clipboard::entry::{ClipboardContent, ClipboardEntry};

// ── Color palette ─────────────────────────────────────────────────────────────

pub const PALETTE: &[(&str, &str)] = &[
    ("red",    "#f38ba8"),
    ("pink",   "#f2cdcd"),
    ("mauve",  "#cba6f7"),
    ("blue",   "#89b4fa"),
    ("teal",   "#94e2d5"),
    ("green",  "#a6e3a1"),
    ("yellow", "#f9e2af"),
    ("peach",  "#fab387"),
];

// ── Icon sets ─────────────────────────────────────────────────────────────────

struct Icons {
    pin_off:  &'static str,
    pin_on:   &'static str,
    delete:   &'static str,
    copy:     &'static str,
    terminal: &'static str,
}

/// Standard Unicode — works with every font.
const ICONS_UNICODE: Icons = Icons {
    pin_off:  "○",   // U+25CB  WHITE CIRCLE
    pin_on:   "●",   // U+25CF  BLACK CIRCLE
    delete:   "✕",   // U+2715  MULTIPLICATION X
    copy:     "⎘",   // U+2398  HELM SYMBOL
    terminal: "⌨",   // U+2328  KEYBOARD
};

/// Nerd Font (Material Design) icons — requires a Nerd Font to be installed.
const ICONS_NERD: Icons = Icons {
    pin_off:  "󰐃",   // nf-md-pin_outline
    pin_on:   "󰐄",   // nf-md-pin
    delete:   "󰗨",   // nf-md-trash_can
    copy:     "󰆏",   // nf-md-content_copy
    terminal: "󰆍",   // nf-md-console
};

#[derive(Clone)]
pub enum RowAction {
    Select,
    Copy,
    TerminalPaste,
    Remove,
    TogglePin(bool), // new pinned state after toggle
    SetLabel { label: Option<String>, color: Option<String> },
}

pub fn build_item_row(
    entry:          &ClipboardEntry,
    nerd_font:      bool,
    suppress_close: Rc<Cell<u32>>,
    on_action:      impl Fn(RowAction) + 'static,
) -> ListBoxRow {
    let icons = if nerd_font { &ICONS_NERD } else { &ICONS_UNICODE };

    let row = ListBoxRow::new();
    row.add_css_class("item-row");
    if entry.pinned {
        row.add_css_class("item-row-pinned");
    }
    if let Some(color) = &entry.color {
        row.add_css_class(&format!("item-row-color-{color}"));
    }

    let on_action: Rc<dyn Fn(RowAction)> = Rc::new(on_action);

    // ── Main horizontal layout ──────────────────────────────────────────
    let hbox = gtk4::Box::new(Orientation::Horizontal, 6);

    // ── Pin indicator ───────────────────────────────────────────────────
    if entry.pinned {
        let pin_indicator = Label::new(Some(icons.pin_on));
        pin_indicator.add_css_class("pin-indicator");
        hbox.append(&pin_indicator);
    }

    // ── Preview + optional label tag (vertical box) ─────────────────────
    let text_box = gtk4::Box::new(Orientation::Vertical, 2);
    text_box.set_hexpand(true);
    text_box.set_halign(gtk4::Align::Fill);

    match &entry.content {
        ClipboardContent::Text(_) => {
            let preview = Label::new(Some(&entry.preview()));
            preview.add_css_class("preview-label");
            preview.set_halign(gtk4::Align::Start);
            preview.set_xalign(0.0);
            preview.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            preview.set_max_width_chars(48);
            text_box.append(&preview);
        }
        ClipboardContent::Image { hash, .. } => {
            // Small "Image" badge above the thumbnail
            let badge = Label::new(Some("Image"));
            badge.add_css_class("image-type-badge");
            badge.set_halign(gtk4::Align::Start);
            text_box.append(&badge);

            // Thumbnail (pre-generated at capture time)
            let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
            let thumb_path = dirs::data_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("clipboard-manager")
                .join("images")
                .join(format!("{hex}_thumb.png"));

            let picture = gtk4::Picture::for_filename(&thumb_path);
            picture.set_can_shrink(true);
            picture.set_size_request(240, 135);
            picture.add_css_class("thumbnail-preview");
            text_box.append(&picture);
        }
    }

    if let Some(lbl) = &entry.label {
        let label_tag = Label::new(Some(lbl));
        label_tag.add_css_class("label-tag");
        label_tag.set_halign(gtk4::Align::Start);
        label_tag.set_xalign(0.0);
        label_tag.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        text_box.append(&label_tag);
    }

    // ── Time label ──────────────────────────────────────────────────────
    let time_label = Label::new(Some(&relative_time(entry.copied_at)));
    time_label.add_css_class("time-label");
    time_label.set_halign(gtk4::Align::End);
    time_label.set_valign(gtk4::Align::Center);

    // ── Action buttons (copy + terminal-paste + pin + delete) ───────────
    let btn_box = gtk4::Box::new(Orientation::Horizontal, 2);
    btn_box.add_css_class("row-actions");

    // Copy button — copies to clipboard without pasting
    let copy_btn = Button::with_label(icons.copy);
    copy_btn.add_css_class("row-btn");
    copy_btn.add_css_class("copy-btn");
    copy_btn.set_tooltip_text(Some("Copy only (no paste)"));

    // Terminal paste button — pastes via Ctrl+Shift+V (hidden for image entries)
    let term_btn = Button::with_label(icons.terminal);
    term_btn.add_css_class("row-btn");
    term_btn.add_css_class("term-btn");
    term_btn.set_tooltip_text(Some("Paste to terminal (Ctrl+Shift+V)"));
    if entry.is_image() {
        term_btn.set_visible(false);
    }

    // Pin toggle button
    let pin_label = if entry.pinned { icons.pin_on } else { icons.pin_off };
    let pin_btn = Button::with_label(pin_label);
    pin_btn.add_css_class("row-btn");
    pin_btn.add_css_class(if entry.pinned { "pin-btn-active" } else { "pin-btn" });
    pin_btn.set_tooltip_text(Some(if entry.pinned { "Unpin" } else { "Pin" }));

    // Delete button
    let del_btn = Button::with_label(icons.delete);
    del_btn.add_css_class("row-btn");
    del_btn.add_css_class("del-btn");
    del_btn.set_tooltip_text(Some("Remove"));

    btn_box.append(&copy_btn);
    btn_box.append(&term_btn);
    btn_box.append(&pin_btn);
    btn_box.append(&del_btn);

    hbox.append(&text_box);
    hbox.append(&time_label);
    hbox.append(&btn_box);

    row.set_child(Some(&hbox));

    // ── Wire up callbacks ───────────────────────────────────────────────
    let id = entry.id;
    let currently_pinned = entry.pinned;
    let entry_label = entry.label.clone();
    let entry_color = entry.color.clone();

    // Click on row body → Select (copy + paste) — button 1 only
    let cb_select = Rc::clone(&on_action);
    let gesture = GestureClick::new();
    gesture.set_button(1);
    gesture.connect_released(move |_, _, _, _| {
        cb_select(RowAction::Select);
    });
    row.add_controller(gesture);

    // Copy button → Copy only (no paste)
    let cb_copy = Rc::clone(&on_action);
    copy_btn.connect_clicked(move |_| {
        cb_copy(RowAction::Copy);
    });

    // Terminal paste button → paste via Ctrl+Shift+V
    let cb_term = Rc::clone(&on_action);
    term_btn.connect_clicked(move |_| {
        cb_term(RowAction::TerminalPaste);
    });

    // Pin button
    let cb_pin = Rc::clone(&on_action);
    pin_btn.connect_clicked(move |_| {
        cb_pin(RowAction::TogglePin(!currently_pinned));
    });

    // Delete button
    let cb_del = Rc::clone(&on_action);
    del_btn.connect_clicked(move |_| {
        cb_del(RowAction::Remove);
    });

    // Right-click → label popover
    {
        let cb_lbl = Rc::clone(&on_action);
        let row_c  = row.clone();
        let sc_c   = Rc::clone(&suppress_close);
        let rc_gesture = GestureClick::new();
        rc_gesture.set_button(3);

        rc_gesture.connect_released(move |_, _, _, _| {
            build_label_popover(
                &row_c,
                entry_label.clone(),
                entry_color.clone(),
                Rc::clone(&cb_lbl),
                Rc::clone(&sc_c),
            );
        });

        row.add_controller(rc_gesture);
    }

    // Store entry id for reference (content lives in row_data in popup.rs)
    unsafe {
        row.set_data("entry-id", id);
    }

    row
}

// ── Right-click label popover ─────────────────────────────────────────────────

fn build_label_popover(
    row:            &ListBoxRow,
    entry_label:    Option<String>,
    entry_color:    Option<String>,
    cb:             Rc<dyn Fn(RowAction)>,
    suppress_close: Rc<Cell<u32>>,
) {
    let committed: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let selected_color: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(entry_color.clone()));
    // Tracks the currently-active swatch button so we can remove its CSS class.
    let active_swatch: Rc<RefCell<Option<Button>>> = Rc::new(RefCell::new(None));

    // ── Popover outer layout ────────────────────────────────────────────
    let vbox = gtk4::Box::new(Orientation::Vertical, 8);
    vbox.set_margin_top(10);
    vbox.set_margin_bottom(10);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);

    // ── Title row ───────────────────────────────────────────────────────
    let title_row = gtk4::Box::new(Orientation::Horizontal, 6);

    let title_lbl = Label::new(Some("Title"));
    title_lbl.add_css_class("popover-form-label");
    title_lbl.set_valign(gtk4::Align::Center);

    let title_entry = Entry::new();
    title_entry.set_placeholder_text(Some("Label…"));
    title_entry.set_text(entry_label.as_deref().unwrap_or(""));
    title_entry.set_hexpand(true);

    title_row.append(&title_lbl);
    title_row.append(&title_entry);
    vbox.append(&title_row);

    // ── Color row ───────────────────────────────────────────────────────
    let color_row = gtk4::Box::new(Orientation::Horizontal, 4);
    color_row.set_margin_top(2);

    let color_lbl = Label::new(Some("Color"));
    color_lbl.add_css_class("popover-form-label");
    color_lbl.set_valign(gtk4::Align::Center);
    color_row.append(&color_lbl);

    // Palette swatches
    for (name, _hex) in PALETTE {
        let swatch = Button::new();
        swatch.add_css_class("color-swatch");
        swatch.add_css_class(&format!("color-swatch-{name}"));
        swatch.set_tooltip_text(Some(name));

        if entry_color.as_deref() == Some(name) {
            swatch.add_css_class("color-swatch-active");
            *active_swatch.borrow_mut() = Some(swatch.clone());
        }

        let sc     = Rc::clone(&selected_color);
        let as_    = Rc::clone(&active_swatch);
        let name_s = name.to_string();
        let sw_c   = swatch.clone();

        swatch.connect_clicked(move |_| {
            // Deactivate previous swatch (clone so we don't hold the borrow)
            let prev = as_.borrow().clone();
            if let Some(p) = prev {
                p.remove_css_class("color-swatch-active");
            }
            sw_c.add_css_class("color-swatch-active");
            *as_.borrow_mut() = Some(sw_c.clone());
            *sc.borrow_mut() = Some(name_s.clone());
        });

        color_row.append(&swatch);
    }

    // "none" button
    let none_btn = Button::with_label("none");
    none_btn.add_css_class("color-swatch");
    none_btn.add_css_class("color-swatch-none");
    none_btn.set_tooltip_text(Some("No color"));

    if entry_color.is_none() {
        none_btn.add_css_class("color-swatch-active");
        *active_swatch.borrow_mut() = Some(none_btn.clone());
    }

    {
        let sc_n   = Rc::clone(&selected_color);
        let as_n   = Rc::clone(&active_swatch);
        let none_c = none_btn.clone();
        none_btn.connect_clicked(move |_| {
            let prev = as_n.borrow().clone();
            if let Some(p) = prev {
                p.remove_css_class("color-swatch-active");
            }
            none_c.add_css_class("color-swatch-active");
            *as_n.borrow_mut() = Some(none_c.clone());
            *sc_n.borrow_mut() = None;
        });
    }

    color_row.append(&none_btn);
    vbox.append(&color_row);

    // ── Apply button ────────────────────────────────────────────────────
    let apply_btn = Button::with_label("Apply");
    apply_btn.add_css_class("apply-btn");
    vbox.append(&apply_btn);

    // ── Build popover ───────────────────────────────────────────────────
    let popover = Popover::new();
    popover.set_child(Some(&vbox));
    popover.set_parent(row);

    // Apply button click → commit + popdown
    {
        let committed_a = Rc::clone(&committed);
        let popover_a   = popover.clone();
        apply_btn.connect_clicked(move |_| {
            committed_a.set(true);
            popover_a.popdown();
        });
    }

    // Enter key in title entry → commit + popdown
    {
        let committed_e = Rc::clone(&committed);
        let popover_e   = popover.clone();
        title_entry.connect_activate(move |_| {
            committed_e.set(true);
            popover_e.popdown();
        });
    }

    // connect_closed: dispatch SetLabel if committed, then release suppress flag.
    // The decrement is deferred by one event-loop tick so that a right-click on
    // another row (which sets suppress += 1 during the same tick) never sees a
    // gap where the counter is 0.
    {
        let committed_c    = Rc::clone(&committed);
        let title_entry_c  = title_entry.clone();
        let sc_c           = Rc::clone(&selected_color);
        let suppress_c     = Rc::clone(&suppress_close);
        popover.connect_closed(move |_| {
            if committed_c.get() {
                let text  = title_entry_c.text().to_string();
                let label = if text.is_empty() { None } else { Some(text) };
                let color = sc_c.borrow().clone();
                cb(RowAction::SetLabel { label, color });
            }
            committed_c.set(false);

            // Decrement after one idle tick (not synchronously) so the window's
            // focus-loss handler never fires between two consecutive popovers or
            // between a popover close and a titlebar drag starting.
            let sc_idle = Rc::clone(&suppress_c);
            glib::idle_add_local_once(move || {
                sc_idle.set(sc_idle.get().saturating_sub(1));
            });
        });
    }

    // Increment before popup() so the counter is > 0 before focus can change.
    suppress_close.set(suppress_close.get() + 1);
    popover.popup();
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn relative_time(copied_at: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let secs = now.saturating_sub(copied_at);
    if secs < 10 {
        "just now".to_string()
    } else if secs < 60 {
        format!("{} sec ago", secs)
    } else if secs < 3600 {
        format!("{} min ago", secs / 60)
    } else if secs < 86400 {
        format!("{} hr ago", secs / 3600)
    } else {
        format!("{} days ago", secs / 86400)
    }
}
