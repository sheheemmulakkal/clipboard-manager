//! Right-click menu of a history row, with "Add label" and "Change colour"
//! sub-pages that slide in inside the same popover.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{Button, Label, ListBoxRow, Orientation, Popover, Stack};

use crate::clipboard::entry::{ClipboardEntry, EntryMeta};
use crate::events::RowAction;
use crate::ui::icons::{self, Icon};
use crate::ui::popup::{menu_item, menu_separator, track_popover};
use crate::ui::theme::{normalize_color, tag_color, Theme, COLORS, DEFAULT_TAGS};

pub const MAX_TAG_CHARS: usize = 24;

/// Tags offered in "Add label": the defaults, then custom tags in use.
pub fn menu_tags(in_use: &[String]) -> Vec<String> {
    let mut out: Vec<String> = DEFAULT_TAGS.iter().map(|(t, _)| t.to_string()).collect();
    for t in in_use {
        if !out.iter().any(|o| o.eq_ignore_ascii_case(t)) {
            out.push(t.clone());
        }
    }
    out
}

/// Trim and length-limit a user-typed tag; `None` if empty.
pub fn clean_tag(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.chars().take(MAX_TAG_CHARS).collect())
}

/// Open the menu for `entry`, pointing at (`x`, `y`) inside `row`.
pub fn show(
    row:      &ListBoxRow,
    point:    Option<(f64, f64)>,
    entry:    &ClipboardEntry,
    theme:    &Theme,
    tags:     &[String],
    suppress: &Rc<Cell<u32>>,
    emit:     Rc<dyn Fn(RowAction)>,
    on_edit:  Rc<dyn Fn()>,
) {
    let popover = Popover::new();
    popover.add_css_class("cm-menu");
    popover.set_has_arrow(false);
    popover.set_position(gtk4::PositionType::Bottom);
    popover.set_parent(row);
    if let Some((x, y)) = point {
        popover.set_pointing_to(Some(&gdk4::Rectangle::new(x as i32, y as i32, 1, 1)));
    }
    track_popover(&popover, suppress);

    // The chosen action runs after the popover has closed and been removed
    // from the row: the action usually rebuilds the list (destroying the row).
    let pending: Rc<RefCell<Option<Choice>>> = Rc::new(RefCell::new(None));
    {
        let pending = Rc::clone(&pending);
        popover.connect_closed(move |p| {
            let choice = pending.borrow_mut().take();
            let p = p.clone();
            let emit = Rc::clone(&emit);
            let on_edit = Rc::clone(&on_edit);
            glib::idle_add_local_once(move || {
                p.unparent();
                match choice {
                    Some(Choice::Action(a)) => emit(a),
                    Some(Choice::Edit) => on_edit(),
                    None => {}
                }
            });
        });
    }
    let choose: Rc<dyn Fn(Choice)> = {
        let pending = Rc::clone(&pending);
        let popover = popover.clone();
        Rc::new(move |c| {
            *pending.borrow_mut() = Some(c);
            popover.popdown();
        })
    };

    let stack = Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::SlideLeftRight);
    stack.set_transition_duration(150);
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);
    stack.set_interpolate_size(true);

    stack.add_named(&main_page(entry, theme, &stack, &choose), Some("main"));
    stack.add_named(&tags_page(entry, theme, tags, &stack, &choose), Some("tags"));
    stack.add_named(&colors_page(entry, theme, &stack, &choose), Some("colors"));
    stack.set_visible_child_name("main");

    popover.set_child(Some(&stack));
    popover.popup();
}

/// What the user picked in the menu.
#[derive(Clone)]
enum Choice {
    Action(RowAction),
    /// Open the editor (a UI action, not a store change).
    Edit,
}

fn meta(entry: &ClipboardEntry) -> EntryMeta {
    EntryMeta { label: entry.label.clone(), color: entry.color.clone(), tag: entry.tag.clone() }
}

fn main_page(entry: &ClipboardEntry, theme: &Theme, stack: &Stack, choose: &Rc<dyn Fn(Choice)>) -> gtk4::Box {
    let page = gtk4::Box::new(Orientation::Vertical, 0);
    let ic = &theme.icon_muted;

    let item = |icon, label: &str, accel: Option<&str>, action: RowAction| {
        let b = menu_item(icon, label, accel, ic);
        let choose = Rc::clone(choose);
        b.connect_clicked(move |_| choose(Choice::Action(action.clone())));
        b
    };

    page.append(&item(Icon::Copy, "Copy", Some("Ctrl+C"), RowAction::Copy));
    let edit = menu_item(Icon::Pencil, "Edit", Some("Ctrl+E"), ic);
    {
        let choose = Rc::clone(choose);
        edit.connect_clicked(move |_| choose(Choice::Edit));
    }
    page.append(&edit);
    let (pin_icon, pin_label) = if entry.pinned { (Icon::PinFilled, "Unpin") } else { (Icon::Pin, "Pin") };
    page.append(&item(pin_icon, pin_label, Some("Ctrl+P"), RowAction::TogglePin));
    page.append(&submenu_item(Icon::Tag, "Add label", ic, stack, "tags"));
    page.append(&submenu_item(Icon::Palette, "Change colour", ic, stack, "colors"));
    if !entry.is_image() {
        page.append(&item(Icon::Terminal, "Paste to terminal", None, RowAction::PasteTerminal));
    }
    page.append(&menu_separator());
    let del = menu_item(Icon::Trash, "Delete", Some("Delete"), &theme.danger_icon());
    del.add_css_class("danger");
    {
        let choose = Rc::clone(choose);
        del.connect_clicked(move |_| choose(Choice::Action(RowAction::Remove)));
    }
    page.append(&del);
    page
}

fn tags_page(
    entry:  &ClipboardEntry,
    theme:  &Theme,
    tags:   &[String],
    stack:  &Stack,
    choose: &Rc<dyn Fn(Choice)>,
) -> gtk4::Box {
    let page = gtk4::Box::new(Orientation::Vertical, 0);
    page.append(&back_header("Add label", theme, stack));

    for tag in menu_tags(tags) {
        let current = entry.tag.as_deref().is_some_and(|t| t.eq_ignore_ascii_case(&tag));
        let b = choice_item(tag_color(&tag), &tag, current, theme);
        let mut m = meta(entry);
        // Choosing the current tag again removes it.
        m.tag = if current { None } else { Some(tag.clone()) };
        let choose = Rc::clone(choose);
        b.connect_clicked(move |_| choose(Choice::Action(RowAction::SetMeta(m.clone()))));
        page.append(&b);
    }
    if entry.tag.is_some() {
        let b = menu_item(Icon::X, "Remove label", None, &theme.icon_muted);
        let mut m = meta(entry);
        m.tag = None;
        let choose = Rc::clone(choose);
        b.connect_clicked(move |_| choose(Choice::Action(RowAction::SetMeta(m.clone()))));
        page.append(&b);
    }

    page.append(&menu_separator());
    let new_btn = menu_item(Icon::Plus, "New label\u{2026}", None, &theme.icon_muted);
    let new_entry = gtk4::Entry::new();
    new_entry.set_placeholder_text(Some("Label name"));
    new_entry.set_max_length(MAX_TAG_CHARS as i32);
    new_entry.set_margin_start(8);
    new_entry.set_margin_end(8);
    new_entry.set_margin_top(4);
    new_entry.set_margin_bottom(4);
    new_entry.set_visible(false);
    {
        let e = new_entry.clone();
        new_btn.connect_clicked(move |b| {
            b.set_visible(false);
            e.set_visible(true);
            e.grab_focus();
        });
    }
    {
        let m = meta(entry);
        let choose = Rc::clone(choose);
        new_entry.connect_activate(move |e| {
            if let Some(tag) = clean_tag(&e.text()) {
                let mut m = m.clone();
                m.tag = Some(tag);
                choose(Choice::Action(RowAction::SetMeta(m)));
            }
        });
    }
    page.append(&new_btn);
    page.append(&new_entry);
    page
}

fn colors_page(entry: &ClipboardEntry, theme: &Theme, stack: &Stack, choose: &Rc<dyn Fn(Choice)>) -> gtk4::Box {
    let page = gtk4::Box::new(Orientation::Vertical, 0);
    page.append(&back_header("Change colour", theme, stack));

    let current = entry.color.as_deref().and_then(normalize_color);
    for (name, _) in COLORS {
        let b = choice_item(name, &capitalize(name), current == Some(name), theme);
        let mut m = meta(entry);
        m.color = Some(name.to_string());
        let choose = Rc::clone(choose);
        b.connect_clicked(move |_| choose(Choice::Action(RowAction::SetMeta(m.clone()))));
        page.append(&b);
    }
    page.append(&menu_separator());
    let none = choice_item("none", "None", current.is_none(), theme);
    let mut m = meta(entry);
    m.color = None;
    let choose = Rc::clone(choose);
    none.connect_clicked(move |_| choose(Choice::Action(RowAction::SetMeta(m.clone()))));
    page.append(&none);
    page
}

/// `[icon] label ……… ›` — switches the stack to `page`.
fn submenu_item(icon: Icon, label: &str, icon_color: &str, stack: &Stack, page: &'static str) -> Button {
    let b = menu_item(icon, label, None, icon_color);
    if let Some(row) = b.child().and_downcast::<gtk4::Box>() {
        row.append(&icons::image(Icon::ChevronRight, icon_color, 14));
    }
    let stack = stack.clone();
    b.connect_clicked(move |_| stack.set_visible_child_name(page));
    b
}

/// `‹ Title` — returns to the main page.
fn back_header(title: &str, theme: &Theme, stack: &Stack) -> Button {
    let row = gtk4::Box::new(Orientation::Horizontal, 8);
    row.append(&icons::image(Icon::ChevronLeft, &theme.icon_muted, 14));
    let l = Label::new(Some(title));
    l.add_css_class("menu-header");
    row.append(&l);
    let b = Button::new();
    b.add_css_class("menu-item");
    b.set_child(Some(&row));
    let stack = stack.clone();
    b.connect_clicked(move |_| stack.set_visible_child_name("main"));
    b
}

/// `● name ……… ✓` — a colour or tag choice.
fn choice_item(color: &str, label: &str, checked: bool, theme: &Theme) -> Button {
    let row = gtk4::Box::new(Orientation::Horizontal, 10);
    let dot = gtk4::Box::new(Orientation::Horizontal, 0);
    dot.add_css_class("color-dot");
    dot.add_css_class(&format!("dot-{color}"));
    dot.set_valign(gtk4::Align::Center);
    row.append(&dot);
    let l = Label::new(Some(label));
    l.add_css_class("menu-label");
    l.set_hexpand(true);
    l.set_halign(gtk4::Align::Start);
    row.append(&l);
    if checked {
        row.append(&icons::image(Icon::Check, &theme.accent_icon(), 14));
    }
    let b = Button::new();
    b.add_css_class("menu-item");
    b.set_child(Some(&row));
    b
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_tags_are_defaults_then_custom_without_duplicates() {
        let in_use = vec!["work".to_string(), "Groceries".to_string(), "Groceries".to_string()];
        let tags = menu_tags(&in_use);
        let names: Vec<&str> = tags.iter().map(|t| t.as_str()).collect();
        assert_eq!(names, vec!["Work", "Personal", "Security", "Ideas", "Snippets", "Groceries"]);
    }

    #[test]
    fn new_tag_names_are_cleaned() {
        assert_eq!(clean_tag("  Side project  "), Some("Side project".to_string()));
        assert_eq!(clean_tag("   "), None);
        assert_eq!(clean_tag(&"x".repeat(40)).unwrap().chars().count(), MAX_TAG_CHARS);
    }
}
