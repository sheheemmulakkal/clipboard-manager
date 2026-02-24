use std::rc::Rc;

use glib::Propagation;
use gtk4::prelude::*;
use gtk4::{
    Application, CssProvider, EventControllerKey, ListBox, ScrolledWindow, SelectionMode, Window,
};

use crate::clipboard::ClipboardEntry;
use crate::ui::item_row::build_item_row;

pub struct ClipboardPopup {
    window: Window,
    list_box: ListBox,
}

impl ClipboardPopup {
    pub fn new(app: &Application) -> Self {
        // Load CSS at compile-time; no runtime file path needed.
        let provider = CssProvider::new();
        provider.load_from_string(include_str!("../../assets/style.css"));
        gtk4::style_context_add_provider_for_display(
            &gdk4::Display::default().expect("no GDK display"),
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        let window = Window::builder()
            .application(app)
            .title("Clipboard")
            .decorated(false)
            .resizable(false)
            .default_width(420)
            .default_height(500)
            .build();

        let scrolled = ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .build();

        let list_box = ListBox::new();
        list_box.set_selection_mode(SelectionMode::None);

        scrolled.set_child(Some(&list_box));
        window.set_child(Some(&scrolled));

        // Close on Escape.
        let key_ctrl = EventControllerKey::new();
        let win_esc = window.clone();
        key_ctrl.connect_key_pressed(move |_, key, _, _| {
            if key == gdk4::Key::Escape {
                win_esc.set_visible(false);
                Propagation::Stop
            } else {
                Propagation::Proceed
            }
        });
        window.add_controller(key_ctrl);

        // Close when the window loses focus.
        window.connect_is_active_notify(|win| {
            if !win.is_active() {
                win.set_visible(false);
            }
        });

        Self { window, list_box }
    }

    /// Clears the list and fills it with `entries` newest-first.
    pub fn populate(
        &self,
        entries: &[ClipboardEntry],
        on_select: impl Fn(u64, String) + 'static,
    ) {
        while let Some(child) = self.list_box.first_child() {
            self.list_box.remove(&child);
        }

        let on_select = Rc::new(on_select);
        for entry in entries.iter().rev() {
            let cb = Rc::clone(&on_select);
            let row = build_item_row(entry, move |id, content| cb(id, content));
            self.list_box.append(&row);
        }
    }

    pub fn show_at_cursor(&self) {
        self.window.present();
    }

    pub fn hide(&self) {
        self.window.set_visible(false);
    }
}
