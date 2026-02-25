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
                        eprintln!("[hotkey/wayland] Failed to create tokio runtime: {e}");
                        return;
                    }
                };

                rt.block_on(async move {
                    match run_global_shortcuts_portal(&hotkey, Arc::clone(&cb)).await {
                        Ok(()) => {}
                        Err(e) => {
                            eprintln!("[hotkey/wayland] GlobalShortcuts portal unavailable: {e}");
                            eprintln!("[hotkey/wayland] Trying evdev keyboard listener...");
                            if !crate::hotkey::evdev::start(&hotkey, cb) {
                                // evdev failed too (no `input` group) — auto-configure a
                                // GNOME custom keyboard shortcut that sends SIGUSR1.
                                setup_gnome_shortcut(&hotkey);
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

    eprintln!(
        "[hotkey/wayland] Registered via GlobalShortcuts portal ({})",
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
            "super" | "win"    => mods.push_str("<Super>"),
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
fn setup_gnome_shortcut(hotkey: &str) {
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "clipboard-manager".to_string());

    let path = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/clipboard-manager/";
    let schema = format!(
        "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:{}",
        path
    );
    let binding = to_portal_trigger(hotkey).unwrap_or_else(|| "<Ctrl><Alt>c".to_string());

    let ok = gsettings(&["set", &schema, "name",    "Clipboard Manager"])
          && gsettings(&["set", &schema, "command", &exe])
          && gsettings(&["set", &schema, "binding", &binding]);

    if !ok {
        eprintln!("[hotkey/wayland] gsettings unavailable — hotkey could not be configured.");
        eprintln!("[hotkey/wayland] On other DEs, add a custom shortcut that runs:");
        eprintln!("[hotkey/wayland]   pkill -USR1 clipboard-manager");
        eprintln!("[hotkey/wayland] The popup can still be opened via the system tray icon.");
        return;
    }

    add_to_keybindings_list(path);

    eprintln!("[hotkey/wayland] ✓ GNOME keyboard shortcut registered: {binding}");
    eprintln!("[hotkey/wayland]   Command: {exe}");
    eprintln!("[hotkey/wayland]   Press the shortcut to open the clipboard popup.");
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
    let current = current.trim();

    if current.contains(path) {
        return; // already registered — idempotent
    }

    let new_val = if current == "@as []" || current.is_empty() {
        format!("['{}']", path)
    } else {
        // current looks like ['/path/1/', '/path/2/'] — insert before closing bracket
        format!("{}, '{}']", current.trim_end_matches(']'), path)
    };

    gsettings(&[
        "set",
        "org.gnome.settings-daemon.plugins.media-keys",
        "custom-keybindings",
        &new_val,
    ]);
}
