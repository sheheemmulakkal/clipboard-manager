use std::sync::Arc;

use anyhow::Result;
use tokio::runtime::Runtime;

use super::HotkeyManager;

/// Wayland hotkey backend.
///
/// Uses the `org.freedesktop.portal.GlobalShortcuts` XDG portal to register
/// a system-wide shortcut.  This is supported on:
/// * GNOME 45+ (mutter)
/// * KDE Plasma 6+ (kwin)
///
/// If the compositor does not support the portal, a warning is printed and
/// the app continues in tray-only mode — the popup can still be opened via
/// the system tray icon.
pub struct WaylandHotkeyManager {
    hotkey: String,
}

impl WaylandHotkeyManager {
    pub fn new(hotkey: &str) -> Self {
        Self { hotkey: hotkey.to_string() }
    }
}

impl HotkeyManager for WaylandHotkeyManager {
    fn start(&self, on_hotkey: Box<dyn Fn() + Send + Sync + 'static>) -> Result<()> {
        let hotkey = self.hotkey.clone();
        let cb: Arc<dyn Fn() + Send + Sync> = Arc::from(on_hotkey);

        // Run the portal listener on a dedicated OS thread so it can block on
        // the tokio runtime without affecting the GTK main thread.
        std::thread::Builder::new()
            .name("wayland-hotkey".into())
            .spawn(move || {
                let rt = match Runtime::new() {
                    Ok(r)  => r,
                    Err(e) => {
                        tracing::warn!("hotkey/wayland: failed to create tokio runtime: {e}");
                        return;
                    }
                };

                rt.block_on(async move {
                    match run_global_shortcuts_portal(&hotkey, Arc::clone(&cb)).await {
                        Ok(()) => {}
                        Err(e) => {
                            tracing::warn!("hotkey/wayland: GlobalShortcuts portal unavailable: {e}");
                            // GNOME: a custom keyboard shortcut running
                            // `clipboard-manager toggle` needs no permissions.
                            if is_gnome() && setup_gnome_shortcut(&hotkey) {
                                return;
                            }
                            tracing::info!("hotkey/wayland: trying evdev keyboard listener...");
                            if !crate::hotkey::evdev::start(&hotkey, cb) {
                                crate::notify::error(
                                    "Clipboard Manager: no global hotkey",
                                    "Add a keyboard shortcut in your desktop settings that runs:\n  clipboard-manager toggle",
                                );
                            }
                        }
                    }
                });
            })
            .ok();

        Ok(())
    }

    fn stop(&self) {
        // The portal session is owned by the background thread and is cleaned up
        // automatically when the thread exits (i.e., when the process ends).
    }
}

// ── Portal implementation ─────────────────────────────────────────────────────

async fn run_global_shortcuts_portal(
    hotkey: &str,
    cb: Arc<dyn Fn() + Send + Sync>,
) -> ashpd::Result<()> {
    use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
    use futures_util::StreamExt;

    let preferred = to_portal_trigger(hotkey);

    let proxy   = GlobalShortcuts::new().await?;
    let session = proxy.create_session().await?;

    let shortcut = NewShortcut::new("clipboard-open", "Open Clipboard History")
        .preferred_trigger(preferred.as_deref().unwrap_or("<Ctrl><Alt>v"));

    proxy.bind_shortcuts(&session, &[shortcut], &ashpd::WindowIdentifier::default()).await?;

    tracing::info!(
        "hotkey/wayland: registered via GlobalShortcuts portal ({})",
        preferred.as_deref().unwrap_or("<no preferred trigger>"),
    );

    let mut stream = proxy.receive_activated().await?;
    while stream.next().await.is_some() {
        cb();
    }

    Ok(())
}

/// Convert our format `"ctrl+alt+v"` to GTK accelerator format `"<Ctrl><Alt>v"`
/// which the GlobalShortcuts portal uses as a `preferred_trigger` hint.
fn to_portal_trigger(hotkey: &str) -> Option<String> {
    let mut mods = String::new();
    let mut key  = String::new();

    for part in hotkey.split('+') {
        match part.trim().to_lowercase().as_str() {
            "ctrl" | "control" => mods.push_str("<Ctrl>"),
            "alt"              => mods.push_str("<Alt>"),
            "super" | "win" | "meta" => mods.push_str("<Super>"),
            "shift"            => mods.push_str("<Shift>"),
            k                  => key = k.to_string(),
        }
    }

    if key.is_empty() { None } else { Some(format!("{mods}{key}")) }
}

// ── GNOME custom-shortcut fallback ────────────────────────────────────────────

/// Auto-configure a GNOME keyboard shortcut that re-launches the binary.
/// Called when both the GlobalShortcuts portal and evdev are unavailable.
///
/// Uses `gsettings` to write to:
///   org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:
///   /org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/clipboard-manager/
///
/// The shortcut command is the current executable path. GTK's single-instance
/// mechanism (via D-Bus) routes the second launch back to the running daemon,
/// which shows the popup — no signals or special permissions required.
fn setup_gnome_shortcut(hotkey: &str) -> bool {
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "clipboard-manager".to_string());
    let command = format!("{exe} toggle");

    let path = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/clipboard-manager/";
    let schema = format!(
        "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:{}",
        path
    );
    let binding = to_portal_trigger(hotkey).unwrap_or_else(|| "<Ctrl><Alt>c".to_string());

    let ok = gsettings(&["set", &schema, "name",    "Clipboard Manager"])
          && gsettings(&["set", &schema, "command", &command])
          && gsettings(&["set", &schema, "binding", &binding]);

    if !ok {
        tracing::warn!("hotkey/wayland: gsettings unavailable — cannot add a GNOME shortcut");
        return false;
    }

    add_to_keybindings_list(path);

    tracing::info!("hotkey/wayland: GNOME keyboard shortcut registered: {binding} → {command}");
    true
}

fn is_gnome() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .map(|d| d.split(':').any(|p| p.eq_ignore_ascii_case("gnome")))
        .unwrap_or(false)
}

/// The gsettings `custom-keybindings` list with `path` appended, or `None`
/// if it is already there.
fn add_path_to_list(current: &str, path: &str) -> Option<String> {
    let current = current.trim();
    if current.contains(path) {
        return None;
    }
    Some(if current == "@as []" || current.is_empty() || current == "[]" {
        format!("['{path}']")
    } else {
        format!("{}, '{path}']", current.trim_end_matches(']'))
    })
}

/// Run `gsettings <args>` and return true on success.
fn gsettings(args: &[&str]) -> bool {
    std::process::Command::new("gsettings")
        .args(args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Append `path` to `org.gnome.settings-daemon.plugins.media-keys custom-keybindings`
/// if it is not already present.
fn add_to_keybindings_list(path: &str) {
    let current = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.settings-daemon.plugins.media-keys", "custom-keybindings"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    let Some(new_val) = add_path_to_list(&current, path) else {
        return; // already registered — idempotent
    };

    gsettings(&[
        "set",
        "org.gnome.settings-daemon.plugins.media-keys",
        "custom-keybindings",
        &new_val,
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portal_trigger_format() {
        assert_eq!(to_portal_trigger("ctrl+alt+c").as_deref(), Some("<Ctrl><Alt>c"));
        assert_eq!(to_portal_trigger("super+shift+v").as_deref(), Some("<Super><Shift>v"));
        assert_eq!(to_portal_trigger("ctrl+alt").as_deref(), None);
    }

    #[test]
    fn keybinding_list_update() {
        let p = "/x/clipboard-manager/";
        assert_eq!(add_path_to_list("@as []", p), Some(format!("['{p}']")));
        assert_eq!(add_path_to_list("", p), Some(format!("['{p}']")));
        assert_eq!(
            add_path_to_list("['/a/']", p),
            Some(format!("['/a/', '{p}']"))
        );
        assert_eq!(add_path_to_list(&format!("['/a/', '{p}']"), p), None); // already there
    }
}
