//! System tray icon (StatusNotifierItem via ksni).
//!
//! On GNOME this needs the AppIndicator extension (enabled by default on
//! Ubuntu); without a tray host the icon simply doesn't appear.

use ksni::blocking::TrayMethods;
use ksni::menu::{CheckmarkItem, StandardItem};
use ksni::MenuItem;

use crate::events::{AppEvent, AppSender};

pub struct ClipboardTray {
    sender: AppSender,
    paused: bool,
    hotkey: String,
}

pub type TrayHandle = ksni::blocking::Handle<ClipboardTray>;

/// Start the tray icon on its own thread. `None` if no tray is available.
pub fn spawn(sender: AppSender, hotkey: &str) -> Option<TrayHandle> {
    let tray = ClipboardTray { sender, paused: false, hotkey: hotkey.to_string() };
    match tray.spawn() {
        Ok(handle) => Some(handle),
        Err(e) => {
            tracing::info!("[tray] not available: {e}");
            None
        }
    }
}

/// Reflect the capture pause state in the tray menu.
pub fn set_paused(handle: &TrayHandle, paused: bool) {
    handle.update(|t| t.paused = paused);
}

/// `"ctrl+alt+c"` → `["Control", "Alt", "c"]` (dbusmenu shortcut format).
fn shortcut_keys(hotkey: &str) -> Vec<String> {
    hotkey
        .split('+')
        .map(|part| match part.trim().to_lowercase().as_str() {
            "ctrl" | "control" => "Control".to_string(),
            "alt" => "Alt".to_string(),
            "shift" => "Shift".to_string(),
            "super" | "win" | "meta" => "Super".to_string(),
            key => key.to_string(),
        })
        .collect()
}

impl ClipboardTray {
    fn send(&self, ev: AppEvent) {
        let _ = self.sender.send_blocking(ev);
    }
}

impl ksni::Tray for ClipboardTray {
    fn id(&self) -> String {
        "clipboard-manager".into()
    }

    fn title(&self) -> String {
        "Clipboard Manager".into()
    }

    fn icon_name(&self) -> String {
        "edit-paste".into()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "Clipboard Manager".into(),
            description: if self.paused {
                "Capture paused".into()
            } else {
                format!("Press {} to open", self.hotkey)
            },
            ..Default::default()
        }
    }

    /// Left click toggles the popup.
    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(AppEvent::Toggle { prev_window: None });
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Clipboard Manager".into(),
                icon_name: "edit-paste".into(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Show / Hide".into(),
                icon_name: "view-list-symbolic".into(),
                shortcut: vec![shortcut_keys(&self.hotkey)],
                activate: Box::new(|t: &mut Self| t.send(AppEvent::Toggle { prev_window: None })),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Pause capture".into(),
                checked: self.paused,
                activate: Box::new(|t: &mut Self| t.send(AppEvent::SetPaused(None))),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Settings".into(),
                icon_name: "preferences-system-symbolic".into(),
                activate: Box::new(|t: &mut Self| t.send(AppEvent::OpenSettings)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit-symbolic".into(),
                activate: Box::new(|t: &mut Self| t.send(AppEvent::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkey_to_dbusmenu_shortcut() {
        assert_eq!(shortcut_keys("ctrl+alt+c"), vec!["Control", "Alt", "c"]);
        assert_eq!(shortcut_keys("Super+Shift+V"), vec!["Super", "Shift", "v"]);
        assert_eq!(shortcut_keys("ctrl+space"), vec!["Control", "space"]);
    }
}
