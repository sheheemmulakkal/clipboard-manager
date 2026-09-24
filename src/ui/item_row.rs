//! One row of the history list.
//!
//! Layout (left → right):
//! `● colour dot · kind icon tile | image thumbnail · title / preview ·
//!  tag pill · time (swapped for action buttons on hover) · pin`

use std::cell::Cell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{Button, EventControllerMotion, GestureClick, Label, ListBoxRow, Orientation, Stack};

use crate::clipboard::entry::{ClipboardContent, ClipboardEntry};
use crate::clipboard::kind::ContentKind;
use crate::events::RowAction;
use crate::ui::context_menu;
use crate::ui::editor;
use crate::ui::format;
use crate::ui::preview;
use crate::ui::icons::{self, Icon};
use crate::ui::theme::{normalize_color, tag_color, Theme};

/// Longest tooltip preview of a text entry.
const TOOLTIP_MAX_CHARS: usize = 600;
const TOOLTIP_MAX_LINES: usize = 12;

/// Everything rows need besides the entry itself.
pub struct RowContext {
    pub theme:           Rc<Theme>,
    pub show_timestamps: bool,
    pub suppress_close:  Rc<Cell<u32>>,
    /// Unix time used for "5 min ago".
    pub now:             u64,
    /// Monitor sizes in device pixels; an image of exactly that size is
    /// labelled "Screenshot".
    pub screen_sizes:    Rc<Vec<(u32, u32)>>,
    /// Tags used anywhere in the history (for the "Add label" menu).
    pub tags:            Rc<Vec<String>>,
}

/// A built row plus hooks the popup's keyboard shortcuts use.
pub struct ItemRow {
    pub row:   ListBoxRow,
    pub hooks: RowHooks,
}

#[derive(Clone)]
pub struct RowHooks {
    /// Open the context menu (Menu key / Shift+F10).
    pub open_menu:   Rc<dyn Fn()>,
    /// Open the editor (Ctrl+E).
    pub open_editor: Rc<dyn Fn()>,
    /// Show the quick-paste number instead of the colour dot (Alt held).
    pub show_quick_index: Rc<dyn Fn(bool)>,
    /// Open the full preview (Space).
    pub open_preview: Rc<dyn Fn()>,
}

/// Kind of an entry, including images/screenshots.
pub fn entry_kind(entry: &ClipboardEntry, screen_sizes: &[(u32, u32)]) -> ContentKind {
    match &entry.content {
        ClipboardContent::Text(t) => ContentKind::detect(t),
        ClipboardContent::Image { width, height, .. } => {
            if screen_sizes.contains(&(*width, *height)) {
                ContentKind::Screenshot
            } else {
                ContentKind::Image
            }
        }
    }
}

pub fn build_item_row(
    entry:     &ClipboardEntry,
    index:     usize,
    ctx:       &RowContext,
    on_action: impl Fn(RowAction) + 'static,
) -> ItemRow {
    let theme = &ctx.theme;
    let on_action: Rc<dyn Fn(RowAction)> = Rc::new(on_action);
    let kind = entry_kind(entry, &ctx.screen_sizes);
    let color = entry.color.as_deref().and_then(normalize_color);

    let row = ListBoxRow::new();
    if entry.pinned {
        row.add_css_class("pinned");
    }

    let hbox = gtk4::Box::new(Orientation::Horizontal, 8);
    hbox.add_css_class("item-row");

    // ── Colour dot ──────────────────────────────────────────────────────
    let dot = gtk4::Box::new(Orientation::Horizontal, 0);
    dot.add_css_class("color-dot");
    dot.add_css_class(&format!("dot-{}", color.unwrap_or("none")));
    dot.set_valign(gtk4::Align::Center);
    hbox.append(&dot);

    // Quick-paste number ("1"…"9"), shown in place of the dot while Alt is held.
    let badge = Label::new(Some(&(index + 1).to_string()));
    badge.add_css_class("quick-index");
    badge.set_valign(gtk4::Align::Center);
    badge.set_visible(false);
    hbox.append(&badge);
    let show_quick_index: Rc<dyn Fn(bool)> = {
        let dot = dot.clone();
        Rc::new(move |on| {
            badge.set_visible(on);
            dot.set_visible(!on);
        })
    };

    // ── Kind tile or thumbnail ──────────────────────────────────────────
    match &entry.content {
        ClipboardContent::Text(_) => {
            let tile = gtk4::CenterBox::new();
            tile.add_css_class("kind-tile");
            tile.set_valign(gtk4::Align::Center);
            tile.set_center_widget(Some(&icons::image(icons::for_kind(kind), &theme.icon, 20)));
            hbox.append(&tile);
        }
        ClipboardContent::Image { hash, .. } => {
            let picture = gtk4::Picture::for_filename(crate::paths::thumb_path(hash));
            picture.set_can_shrink(true);
            picture.set_size_request(64, 44);
            picture.add_css_class("thumb-frame");
            picture.set_overflow(gtk4::Overflow::Hidden);
            picture.set_valign(gtk4::Align::Center);
            hbox.append(&picture);
        }
    }

    // ── Title + preview ─────────────────────────────────────────────────
    let text_box = gtk4::Box::new(Orientation::Vertical, 2);
    text_box.set_hexpand(true);
    text_box.set_valign(gtk4::Align::Center);

    let title = Label::new(Some(&format::title(entry, kind)));
    title.add_css_class("row-title");
    title.set_xalign(0.0);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    text_box.append(&title);

    let subtitle = Label::new(Some(&format::subtitle(entry)));
    subtitle.add_css_class("row-subtitle");
    subtitle.set_xalign(0.0);
    subtitle.set_single_line_mode(true);
    subtitle.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    if let ClipboardContent::Text(t) = &entry.content {
        subtitle.set_tooltip_text(Some(&tooltip_preview(t)));
    }
    text_box.append(&subtitle);
    hbox.append(&text_box);

    // ── Tag pill ────────────────────────────────────────────────────────
    if let Some(tag) = entry.tag.as_deref().filter(|t| !t.trim().is_empty()) {
        let pill = Label::new(Some(tag));
        pill.add_css_class("tag-pill");
        pill.add_css_class(&format!("tag-{}", color.unwrap_or_else(|| tag_color(tag))));
        pill.set_valign(gtk4::Align::Center);
        pill.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        pill.set_max_width_chars(12);
        hbox.append(&pill);
    }

    // ── Time ⇄ hover actions ────────────────────────────────────────────
    let stack = Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::Crossfade);
    stack.set_transition_duration(120);
    stack.set_valign(gtk4::Align::Center);

    let time = Label::new(Some(&format::relative_time(entry.copied_at, ctx.now)));
    time.add_css_class("time-label");
    time.set_halign(gtk4::Align::End);
    time.set_visible(ctx.show_timestamps);
    stack.add_named(&time, Some("time"));

    let actions = gtk4::Box::new(Orientation::Horizontal, 2);
    actions.set_halign(gtk4::Align::End);
    let preview_btn = row_button(Icon::Eye, &theme.icon_muted, "Preview (Space)");
    actions.append(&preview_btn);
    let copy_btn = row_button(Icon::Copy, &theme.icon_muted, "Copy (Ctrl+C)");
    actions.append(&copy_btn);
    let term_btn = row_button(Icon::Terminal, &theme.icon_muted, "Paste to terminal (Ctrl+Shift+V)");
    term_btn.set_visible(!entry.is_image());
    actions.append(&term_btn);
    let del_btn = row_button(Icon::Trash, &theme.icon_muted, "Delete (Del)");
    del_btn.add_css_class("danger");
    actions.append(&del_btn);
    stack.add_named(&actions, Some("actions"));
    stack.set_visible_child_name("time");
    hbox.append(&stack);

    // ── Pin ─────────────────────────────────────────────────────────────
    let pin_btn = if entry.pinned {
        row_button(Icon::PinFilled, &theme.accent_icon(), "Unpin")
    } else {
        row_button(Icon::Pin, &theme.icon_muted, "Pin")
    };
    pin_btn.remove_css_class("row-btn");
    pin_btn.add_css_class("pin-toggle");
    hbox.append(&pin_btn);

    row.set_child(Some(&hbox));

    // ── Wire up ─────────────────────────────────────────────────────────
    {
        let motion = EventControllerMotion::new();
        let s = stack.clone();
        motion.connect_enter(move |_, _, _| s.set_visible_child_name("actions"));
        let s = stack.clone();
        motion.connect_leave(move |_| s.set_visible_child_name("time"));
        row.add_controller(motion);
    }

    // Click on the row body → paste (button 1 only).
    {
        let cb = Rc::clone(&on_action);
        let gesture = GestureClick::new();
        gesture.set_button(1);
        gesture.connect_released(move |_, _, _, _| cb(RowAction::Paste));
        row.add_controller(gesture);
    }

    connect(&copy_btn, &on_action, RowAction::Copy);
    connect(&term_btn, &on_action, RowAction::PasteTerminal);
    connect(&del_btn, &on_action, RowAction::Remove);
    connect(&pin_btn, &on_action, RowAction::TogglePin);

    let open_preview: Rc<dyn Fn()> = {
        let row      = row.clone();
        let entry    = entry.clone();
        let suppress = Rc::clone(&ctx.suppress_close);
        let cb       = Rc::clone(&on_action);
        Rc::new(move || preview::show(&row, &entry, &suppress, Rc::clone(&cb)))
    };
    {
        let open = Rc::clone(&open_preview);
        preview_btn.connect_clicked(move |_| open());
    }

    let open_editor: Rc<dyn Fn()> = {
        let row      = row.clone();
        let entry    = entry.clone();
        let suppress = Rc::clone(&ctx.suppress_close);
        let cb       = Rc::clone(&on_action);
        Rc::new(move || editor::show(&row, &entry, &suppress, Rc::clone(&cb)))
    };

    // Context menu: right-click at the pointer, or keyboard at the row.
    let open_menu_at: Rc<dyn Fn(Option<(f64, f64)>)> = {
        let row      = row.clone();
        let entry    = entry.clone();
        let theme    = Rc::clone(&ctx.theme);
        let tags     = Rc::clone(&ctx.tags);
        let suppress = Rc::clone(&ctx.suppress_close);
        let cb       = Rc::clone(&on_action);
        let ui = context_menu::UiHooks {
            edit:    Rc::clone(&open_editor),
            preview: Rc::clone(&open_preview),
        };
        Rc::new(move |point| {
            context_menu::show(&row, point, &entry, &theme, &tags, &suppress, Rc::clone(&cb), ui.clone());
        })
    };
    {
        let open = Rc::clone(&open_menu_at);
        let gesture = GestureClick::new();
        gesture.set_button(3);
        gesture.connect_pressed(move |g, _, x, y| {
            g.set_state(gtk4::EventSequenceState::Claimed);
            open(Some((x, y)));
        });
        row.add_controller(gesture);
    }
    let open_menu: Rc<dyn Fn()> = Rc::new(move || open_menu_at(None));

    ItemRow { row, hooks: RowHooks { open_menu, open_editor, show_quick_index, open_preview } }
}

fn row_button(icon: Icon, color: &str, tooltip: &str) -> Button {
    let b = Button::new();
    b.add_css_class("row-btn");
    b.set_child(Some(&icons::image(icon, color, 16)));
    b.set_tooltip_text(Some(tooltip));
    b.set_valign(gtk4::Align::Center);
    b
}

fn connect(btn: &Button, cb: &Rc<dyn Fn(RowAction)>, action: RowAction) {
    let cb = Rc::clone(cb);
    btn.connect_clicked(move |_| cb(action.clone()));
}

/// First lines of a text entry for the hover tooltip.
fn tooltip_preview(text: &str) -> String {
    let mut out: String = text
        .lines()
        .take(TOOLTIP_MAX_LINES)
        .collect::<Vec<_>>()
        .join("\n");
    if out.chars().count() > TOOLTIP_MAX_CHARS {
        out = out.chars().take(TOOLTIP_MAX_CHARS).collect();
    }
    if out.len() < text.trim_end().len() {
        out.push('\u{2026}');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltip_is_bounded() {
        let many_lines = (0..50).map(|i| i.to_string()).collect::<Vec<_>>().join("\n");
        let t = tooltip_preview(&many_lines);
        assert_eq!(t.lines().count(), TOOLTIP_MAX_LINES);
        assert!(t.ends_with('\u{2026}'));
        assert_eq!(tooltip_preview("short"), "short");
    }

    #[test]
    fn screenshot_is_an_image_of_screen_size() {
        let shot = ClipboardEntry::new_image(1, [0; 32], 1920, 1080);
        let small = ClipboardEntry::new_image(2, [0; 32], 64, 48);
        let screens = [(1920, 1080)];
        assert_eq!(entry_kind(&shot, &screens), ContentKind::Screenshot);
        assert_eq!(entry_kind(&small, &screens), ContentKind::Image);
    }
}
