pub mod x11;

pub use x11::X11HotkeyManager;

pub trait HotkeyManager {
    /// Register the hotkey and start listening. `on_hotkey` is called on the
    /// GTK main thread each time the key combination is pressed.
    fn start(&self, on_hotkey: impl Fn() + Send + Sync + 'static) -> anyhow::Result<()>;
    /// Unregister the hotkey and stop the listener thread.
    fn stop(&self);
}
