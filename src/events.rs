//! Messages that drive the controller.
//!
//! * [`AppEvent`] — sent from any thread (hotkey listener, re-activation)
//!   through an `async_channel` and handled on the GTK main thread.
//! * [`PopupEvent`] — emitted by the popup UI on the main thread.

use crate::clipboard::entry::EntryMeta;

pub enum AppEvent {
    /// Open the popup; `prev_window` is the X11 window focused before it.
    Show { prev_window: Option<u64> },
    /// Pause (`Some(true)`), resume (`Some(false)`) or toggle (`None`) capture.
    SetPaused(Option<bool>),
}

/// Something the user did to one history row.
#[derive(Clone, Debug)]
pub enum RowAction {
    /// Copy to the clipboard, hide the popup, paste into the previous window.
    Paste,
    /// Like `Paste` but with Ctrl+Shift+V (terminals). Text only.
    PasteTerminal,
    /// Copy to the clipboard only.
    Copy,
    Remove,
    TogglePin,
    SetMeta(EntryMeta),
    /// Replace the text of a text entry.
    EditContent(String),
}

pub enum PopupEvent {
    Row(u64, RowAction),
    SearchChanged(String),
    ClearAll,
    Menu(MenuAction),
}

/// Items of the popup's ☰ menu (and later the tray menu).
#[derive(Clone, Copy, Debug)]
pub enum MenuAction {
    TogglePause,
    OpenSettings,
    About,
    Quit,
}
