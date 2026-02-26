use std::time::{SystemTime, UNIX_EPOCH};

use gtk4::prelude::*;
use gtk4::{Button, GestureClick, Label, ListBoxRow, Orientation};

use crate::clipboard::ClipboardEntry;

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
}

pub fn build_item_row(
    entry:     &ClipboardEntry,
    nerd_font: bool,
    on_action: impl Fn(RowAction) + 'static,
) -> ListBoxRow {
    let icons = if nerd_font { &ICONS_NERD } else { &ICONS_UNICODE };

    let row = ListBoxRow::new();
    row.add_css_class("item-row");
    if entry.pinned {
        row.add_css_class("item-row-pinned");
    }

    let on_action = std::rc::Rc::new(on_action);

    // ── Main horizontal layout ──────────────────────────────────────────
    let hbox = gtk4::Box::new(Orientation::Horizontal, 6);

    // ── Pin indicator ───────────────────────────────────────────────────
    if entry.pinned {
        let pin_indicator = Label::new(Some(icons.pin_on));
        pin_indicator.add_css_class("pin-indicator");
        hbox.append(&pin_indicator);
    }

    // ── Preview text ────────────────────────────────────────────────────
    let preview = Label::new(Some(entry.preview()));
    preview.add_css_class("preview-label");
    preview.set_hexpand(true);
    preview.set_halign(gtk4::Align::Start);
    preview.set_xalign(0.0);
    preview.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    preview.set_max_width_chars(48);

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

    // Terminal paste button — pastes via Ctrl+Shift+V
    let term_btn = Button::with_label(icons.terminal);
    term_btn.add_css_class("row-btn");
    term_btn.add_css_class("term-btn");
    term_btn.set_tooltip_text(Some("Paste to terminal (Ctrl+Shift+V)"));

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

    hbox.append(&preview);
    hbox.append(&time_label);
    hbox.append(&btn_box);

    row.set_child(Some(&hbox));

    // ── Wire up callbacks ───────────────────────────────────────────────
    let id = entry.id;
    let currently_pinned = entry.pinned;
    let content = entry.content.clone();

    // Click on row body → Select (copy + paste)
    let cb_select = std::rc::Rc::clone(&on_action);
    let gesture = GestureClick::new();
    gesture.connect_released(move |_, _, _, _| {
        cb_select(RowAction::Select);
    });
    row.add_controller(gesture);

    // Copy button → Copy only (no paste)
    let cb_copy = std::rc::Rc::clone(&on_action);
    copy_btn.connect_clicked(move |_| {
        cb_copy(RowAction::Copy);
    });

    // Terminal paste button → paste via Ctrl+Shift+V
    let cb_term = std::rc::Rc::clone(&on_action);
    term_btn.connect_clicked(move |_| {
        cb_term(RowAction::TerminalPaste);
    });

    // Pin button
    let cb_pin = std::rc::Rc::clone(&on_action);
    pin_btn.connect_clicked(move |_| {
        cb_pin(RowAction::TogglePin(!currently_pinned));
    });

    // Delete button
    let cb_del = std::rc::Rc::clone(&on_action);
    del_btn.connect_clicked(move |_| {
        cb_del(RowAction::Remove);
    });

    // Store id/content for keyboard handler in popup.rs
    unsafe {
        row.set_data("entry-id", id);
        row.set_data("entry-content", content);
    }

    row
}

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
