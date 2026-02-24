use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gdk4::prelude::*;
use glib::ControlFlow;

use crate::clipboard::entry::ClipboardEntry;
use crate::config::AppConfig;
use crate::store::Store;

pub struct ClipboardMonitor {
    #[allow(dead_code)]
    last_text: Rc<RefCell<String>>,
}

impl ClipboardMonitor {
    pub fn start(
        store: Rc<RefCell<Box<dyn Store>>>,
        _config: AppConfig,
        on_change: impl Fn() + 'static,
    ) -> Self {
        let last_text = Rc::new(RefCell::new(String::new()));
        let on_change = Rc::new(on_change);

        // GDK clipboard is backend-agnostic: works on both X11 and Wayland.
        let clipboard = gdk4::Display::default()
            .expect("no GDK display")
            .clipboard();

        glib::timeout_add_local(Duration::from_millis(500), {
            let last_text = Rc::clone(&last_text);
            let store = Rc::clone(&store);
            let on_change = Rc::clone(&on_change);

            move || {
                // Clone Rc handles so each async callback owns its own references.
                let last_text = Rc::clone(&last_text);
                let store = Rc::clone(&store);
                let on_change = Rc::clone(&on_change);

                clipboard.read_text_async(
                    None::<&gdk4::gio::Cancellable>,
                    move |result: Result<Option<glib::GString>, glib::Error>| {
                        if let Ok(Some(text)) = result {
                            let text = text.to_string();
                            if text.is_empty() {
                                return;
                            }

                            let last = last_text.borrow().clone();
                            if text != last {
                                *last_text.borrow_mut() = text.clone();

                                eprintln!("[monitor] captured: {:?}", &text[..text.len().min(60)]);
                                let next_id = store.borrow().len() as u64 + 1;
                                let entry = ClipboardEntry::new(next_id, text);
                                store.borrow_mut().add(entry);

                                on_change();
                            }
                        }
                    },
                );

                ControlFlow::Continue
            }
        });

        Self { last_text }
    }
}
