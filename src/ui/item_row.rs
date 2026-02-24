use std::time::{SystemTime, UNIX_EPOCH};

use gtk4::prelude::*;
use gtk4::{GestureClick, Label, ListBoxRow, Orientation};

use crate::clipboard::ClipboardEntry;

pub fn build_item_row(
    entry: &ClipboardEntry,
    on_select: impl Fn(u64, String) + 'static,
) -> ListBoxRow {
    let row = ListBoxRow::new();
    row.add_css_class("item-row");

    let hbox = gtk4::Box::new(Orientation::Horizontal, 8);

    let preview = Label::new(Some(entry.preview()));
    preview.add_css_class("preview-label");
    preview.set_hexpand(true);
    preview.set_halign(gtk4::Align::Start);
    preview.set_xalign(0.0);
    preview.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    preview.set_max_width_chars(55);

    let time_label = Label::new(Some(&relative_time(entry.copied_at)));
    time_label.add_css_class("time-label");
    time_label.set_halign(gtk4::Align::End);
    time_label.set_valign(gtk4::Align::Center);

    hbox.append(&preview);
    hbox.append(&time_label);
    row.set_child(Some(&hbox));

    let id = entry.id;
    let content = entry.content.clone();

    let gesture = GestureClick::new();
    gesture.connect_released(move |_, _, _, _| {
        on_select(id, content.clone());
    });
    row.add_controller(gesture);

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
