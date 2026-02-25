pub mod evdev;
pub mod wayland;
pub mod x11;

pub use wayland::WaylandHotkeyManager;
pub use x11::X11HotkeyManager;

/// Platform-agnostic hotkey listener.
///
/// Using `Box<dyn Fn()>` in the signature (instead of `impl Fn()`) keeps the
/// trait dyn-compatible so `Box<dyn HotkeyManager>` can be used as a
/// type-erased return from `detect()`.
pub trait HotkeyManager: 'static {
    /// Register the hotkey and start listening.
    /// `on_hotkey` is called each time the combination is pressed.
    fn start(&self, on_hotkey: Box<dyn Fn() + Send + Sync + 'static>) -> anyhow::Result<()>;
    /// Unregister the hotkey and stop the listener thread.
    fn stop(&self);
}

/// Return the best `HotkeyManager` for the current display server.
///
/// * Wayland (`WAYLAND_DISPLAY` set) → `WaylandHotkeyManager`
///   (uses `org.freedesktop.portal.GlobalShortcuts`; falls back gracefully
///    if the compositor does not support the portal)
/// * X11 → `X11HotkeyManager` (XGrabKey via x11rb)
pub fn detect(hotkey: &str) -> Box<dyn HotkeyManager> {
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        eprintln!("[hotkey] Wayland session — using GlobalShortcuts backend");
        Box::new(WaylandHotkeyManager::new(hotkey))
    } else {
        eprintln!("[hotkey] X11 session — using X11 backend");
        Box::new(X11HotkeyManager::new(hotkey))
    }
}
