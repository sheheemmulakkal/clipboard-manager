//! "Edit" popover: title (label) and, for text entries, the content.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{Button, Entry, Label, ListBoxRow, Orientation, Popover, TextView};

use crate::clipboard::entry::{ClipboardContent, ClipboardEntry, EntryMeta};
use crate::events::RowAction;
use crate::ui::popup::track_popover;

/// The row actions an edit results in (empty if nothing changed).
pub fn edits(entry: &ClipboardEntry, title: &str, text: Option<&str>) -> Vec<RowAction> {
    let mut out = Vec::new();
    let title = title.trim();
    let label = (!title.is_empty()).then(|| title.to_string());
    if label != entry.label {
        out.push(RowAction::SetMeta(EntryMeta {
            label,
            color: entry.color.clone(),
            tag:   entry.tag.clone(),
        }));
    }
    if let (Some(new), ClipboardContent::Text(old)) = (text, &entry.content) {
        if new != old && !new.trim().is_empty() {
            out.push(RowAction::EditContent(new.to_string()));
        }
    }
    out
}

/// Open the editor for `entry`, anchored to `row`.
pub fn show(row: &ListBoxRow, entry: &ClipboardEntry, suppress: &Rc<Cell<u32>>, emit: Rc<dyn Fn(RowAction)>) {
    let popover = Popover::new();
    popover.add_css_class("cm-editor");
    popover.set_has_arrow(false);
    popover.set_position(gtk4::PositionType::Bottom);
    popover.set_parent(row);
    track_popover(&popover, suppress);

    let vbox = gtk4::Box::new(Orientation::Vertical, 6);
    vbox.set_size_request(360, -1);

    let title_lbl = Label::new(Some("Title"));
    title_lbl.add_css_class("popover-form-label");
    title_lbl.set_xalign(0.0);
    let title = Entry::new();
    title.set_placeholder_text(Some("Optional — shown instead of the content type"));
    title.set_text(entry.label.as_deref().unwrap_or(""));
    vbox.append(&title_lbl);
    vbox.append(&title);

    let content_view = match &entry.content {
        ClipboardContent::Text(t) => {
            let lbl = Label::new(Some("Content"));
            lbl.add_css_class("popover-form-label");
            lbl.set_xalign(0.0);
            lbl.set_margin_top(6);
            let view = TextView::new();
            view.set_monospace(true);
            view.set_wrap_mode(gtk4::WrapMode::WordChar);
            view.set_left_margin(8);
            view.set_right_margin(8);
            view.set_top_margin(6);
            view.set_bottom_margin(6);
            view.buffer().set_text(t);
            let scroll = gtk4::ScrolledWindow::builder()
                .hscrollbar_policy(gtk4::PolicyType::Never)
                .min_content_height(160)
                .max_content_height(320)
                .propagate_natural_height(true)
                .child(&view)
                .build();
            scroll.add_css_class("editor-scroll");
            vbox.append(&lbl);
            vbox.append(&scroll);
            Some(view)
        }
        ClipboardContent::Image { .. } => None,
    };

    let buttons = gtk4::Box::new(Orientation::Horizontal, 8);
    buttons.set_halign(gtk4::Align::End);
    buttons.set_margin_top(8);
    let hint = Label::new(Some("Ctrl+Enter to save"));
    hint.add_css_class("popover-form-label");
    hint.set_hexpand(true);
    hint.set_xalign(0.0);
    let cancel = Button::with_label("Cancel");
    cancel.add_css_class("secondary-btn");
    let save = Button::with_label("Save");
    save.add_css_class("primary-btn");
    buttons.append(&hint);
    buttons.append(&cancel);
    buttons.append(&save);
    vbox.append(&buttons);
    popover.set_child(Some(&vbox));

    // Actions are emitted once the popover has closed (see context_menu).
    let pending: Rc<RefCell<Vec<RowAction>>> = Rc::new(RefCell::new(Vec::new()));
    {
        let pending = Rc::clone(&pending);
        popover.connect_closed(move |p| {
            let actions = std::mem::take(&mut *pending.borrow_mut());
            let p = p.clone();
            let emit = Rc::clone(&emit);
            glib::idle_add_local_once(move || {
                p.unparent();
                for a in actions {
                    emit(a);
                }
            });
        });
    }

    let commit: Rc<dyn Fn()> = {
        let entry = entry.clone();
        let title = title.clone();
        let view = content_view.clone();
        let pending = Rc::clone(&pending);
        let popover = popover.clone();
        Rc::new(move || {
            let text = view.as_ref().map(|v| {
                let b = v.buffer();
                b.text(&b.start_iter(), &b.end_iter(), false).to_string()
            });
            *pending.borrow_mut() = edits(&entry, &title.text(), text.as_deref());
            popover.popdown();
        })
    };

    {
        let c = Rc::clone(&commit);
        save.connect_clicked(move |_| c());
    }
    {
        let p = popover.clone();
        cancel.connect_clicked(move |_| p.popdown());
    }
    {
        let c = Rc::clone(&commit);
        title.connect_activate(move |_| c());
    }
    {
        let keys = gtk4::EventControllerKey::new();
        keys.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let c = Rc::clone(&commit);
        keys.connect_key_pressed(move |_, key, _, mods| {
            if mods.contains(gdk4::ModifierType::CONTROL_MASK)
                && (key == gdk4::Key::Return || key == gdk4::Key::KP_Enter)
            {
                c();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        popover.add_controller(keys);
    }

    popover.popup();
    title.grab_focus();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::entry::ClipboardEntry;

    fn entry() -> ClipboardEntry {
        let mut e = ClipboardEntry::new_text(1, "body".into());
        e.label = Some("Title".into());
        e.color = Some("red".into());
        e.tag = Some("Work".into());
        e
    }

    #[test]
    fn unchanged_edit_does_nothing() {
        assert!(edits(&entry(), "Title", Some("body")).is_empty());
    }

    #[test]
    fn title_change_keeps_colour_and_tag() {
        let a = edits(&entry(), "  New  ", Some("body"));
        assert_eq!(a.len(), 1);
        match &a[0] {
            RowAction::SetMeta(m) => {
                assert_eq!(m.label.as_deref(), Some("New"));
                assert_eq!(m.color.as_deref(), Some("red"));
                assert_eq!(m.tag.as_deref(), Some("Work"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn blank_title_clears_label() {
        match &edits(&entry(), "  ", Some("body"))[0] {
            RowAction::SetMeta(m) => assert_eq!(m.label, None),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn content_change_is_an_edit_and_empty_content_is_ignored() {
        let a = edits(&entry(), "Title", Some("changed"));
        assert!(matches!(&a[..], [RowAction::EditContent(t)] if t == "changed"));
        assert!(edits(&entry(), "Title", Some("   ")).is_empty());
    }
}
