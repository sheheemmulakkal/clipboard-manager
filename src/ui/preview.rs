//! Full-content preview popover (text or image).

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{Button, Label, ListBoxRow, Orientation, Popover};

use crate::clipboard::entry::{ClipboardContent, ClipboardEntry};
use crate::events::RowAction;
use crate::ui::format;
use crate::ui::popup::{dispose_popover, track_popover, CloseGuard};

/// More than this is not rendered (a label with megabytes of text is slow).
const PREVIEW_MAX_CHARS: usize = 200_000;

pub fn show(row: &ListBoxRow, entry: &ClipboardEntry, suppress: &Rc<CloseGuard>, emit: Rc<dyn Fn(RowAction)>) {
    let popover = Popover::new();
    popover.add_css_class("cm-editor");
    popover.set_has_arrow(false);
    popover.set_position(gtk4::PositionType::Bottom);
    popover.set_parent(row);
    track_popover(&popover, suppress);
    popover.add_weak_ref_notify_local(|| tracing::trace!("[popover] freed"));

    let vbox = gtk4::Box::new(Orientation::Vertical, 8);
    vbox.set_size_request(400, -1);

    let file_size = match &entry.content {
        ClipboardContent::Image { hash, .. } => {
            std::fs::metadata(crate::paths::image_path(hash)).ok().map(|m| m.len())
        }
        ClipboardContent::Text(_) => None,
    };
    let header = Label::new(Some(&format::preview_header(entry, file_size)));
    header.add_css_class("popover-form-label");
    header.set_xalign(0.0);
    vbox.append(&header);

    let mut text_label = None;
    match &entry.content {
        ClipboardContent::Text(t) => {
            let (body, note) = format::preview_body(t, PREVIEW_MAX_CHARS);
            let text = Label::new(Some(&body));
            text.add_css_class("preview-text");
            text.set_xalign(0.0);
            text.set_yalign(0.0);
            text.set_wrap(true);
            text.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
            text.set_selectable(true);
            text.set_margin_start(8);
            text.set_margin_end(8);
            text.set_margin_top(6);
            text.set_margin_bottom(6);
            let scroll = gtk4::ScrolledWindow::builder()
                .hscrollbar_policy(gtk4::PolicyType::Never)
                .max_content_height(320)
                .propagate_natural_height(true)
                .child(&text)
                .build();
            scroll.add_css_class("editor-scroll");
            vbox.append(&scroll);
            text_label = Some(text);
            if let Some(note) = note {
                let n = Label::new(Some(&note));
                n.add_css_class("popover-form-label");
                n.set_xalign(0.0);
                vbox.append(&n);
            }
        }
        ClipboardContent::Image { hash, width, height } => {
            // Fit inside 400 × 300, keeping the aspect ratio. Load a scaled
            // copy: a Picture's natural size is its image's size.
            let scale = (400.0 / *width as f64).min(300.0 / *height as f64).min(1.0);
            let (w, h) = (
                ((*width as f64 * scale) as i32).max(1),
                ((*height as f64 * scale) as i32).max(1),
            );
            let picture = match gdk_pixbuf::Pixbuf::from_file_at_scale(crate::paths::image_path(hash), w, h, true) {
                #[allow(deprecated)] // Texture::for_pixbuf is fine on GTK 4.6–4.14
                Ok(pb) => gtk4::Picture::for_paintable(&gdk4::Texture::for_pixbuf(&pb)),
                Err(e) => {
                    tracing::warn!("[preview] cannot load image: {e}");
                    gtk4::Picture::new()
                }
            };
            picture.set_size_request(w, h);
            picture.add_css_class("thumb-frame");
            picture.set_overflow(gtk4::Overflow::Hidden);
            // A Picture grows to the width it is given; Fixed allocates
            // exactly its natural size.
            let frame = gtk4::Fixed::new();
            frame.put(&picture, 0.0, 0.0);
            frame.set_halign(gtk4::Align::Center);
            vbox.append(&frame);
        }
    }

    let buttons = gtk4::Box::new(Orientation::Horizontal, 8);
    buttons.set_halign(gtk4::Align::End);
    let copy = Button::with_label("Copy");
    copy.add_css_class("secondary-btn");
    let paste = Button::with_label("Paste");
    paste.add_css_class("primary-btn");
    buttons.append(&copy);
    buttons.append(&paste);
    vbox.append(&buttons);
    popover.set_child(Some(&vbox));

    // Actions run after the popover closed and left the row (see context_menu).
    let pending: Rc<RefCell<Option<RowAction>>> = Rc::new(RefCell::new(None));
    {
        let pending = Rc::clone(&pending);
        popover.connect_closed(move |p| {
            let action = pending.borrow_mut().take();
            let p = p.clone();
            let emit = Rc::clone(&emit);
            glib::idle_add_local_once(move || {
                dispose_popover(&p);
                if let Some(a) = action {
                    emit(a);
                }
            });
        });
    }
    for (btn, action) in [(&copy, RowAction::Copy), (&paste, RowAction::Paste)] {
        let pending = Rc::clone(&pending);
        let p = popover.clone();
        btn.connect_clicked(move |_| {
            *pending.borrow_mut() = Some(action.clone());
            p.popdown();
        });
    }
    {
        // Space closes the preview again (Quick Look style). Capture phase:
        // otherwise the focused button would treat Space as a click.
        let keys = gtk4::EventControllerKey::new();
        keys.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let p = popover.downgrade();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk4::Key::space {
                if let Some(p) = p.upgrade() {
                    p.popdown();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        popover.add_controller(keys);
    }

    popover.popup();
    // After the popover's own initial focus (which would select all text
    // in the label): focus Paste and clear any selection.
    glib::idle_add_local_once(move || {
        paste.grab_focus();
        if let Some(label) = text_label {
            label.select_region(0, 0);
        }
    });
}
