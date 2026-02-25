pub mod wayland;
pub mod x11;

pub use wayland::WaylandPlatform;
pub use x11::X11Platform;

/// Platform-specific backend.  All methods are called from the GTK main thread
/// unless stated otherwise.  Implementations must be `Send + Sync` so they can
/// be wrapped in `Arc` and passed into background threads.
pub trait Platform: Send + Sync {
    /// Capture the currently active X11 window ID *before* the popup is shown.
    ///
    /// Called from the hotkey background thread.
    /// Returns `None` on Wayland (no active-window API).
    fn capture_active_window(&self) -> Option<u64>;

    /// Paste clipboard contents.
    ///
    /// * X11  – activates `prev_window` (if `Some`) then sends Ctrl+V via XTest.
    /// * Wayland – sends Ctrl+V to the currently focused window via the
    ///   RemoteDesktop portal.
    ///
    /// Called from a dedicated `std::thread::spawn` thread in app.rs; may block.
    fn paste(&self, prev_window: Option<u64>);

    /// Return the cursor's current screen coordinates.
    /// Returns `None` on Wayland (compositor controls window placement).
    fn cursor_position(&self) -> Option<(i32, i32)>;

    /// Move the GTK4 popup window to screen position (x, y).
    ///
    /// * X11     – obtains the X11 window ID via `gdk4-x11` and calls
    ///             `configure_window`.
    /// * Wayland – no-op; the compositor positions the window.
    fn move_popup(&self, window: &gtk4::Window, x: i32, y: i32);

    /// Returns `true` if mouse button 1 is physically held down.
    /// Used by the drag-detection poll in popup.rs.
    /// Always returns `false` on Wayland.
    fn button1_held(&self) -> bool;

    /// Returns `true` if `button1_held()` can actually reflect hardware state.
    /// On Wayland this is `false`; the compositor never exposes button state.
    /// The popup drag-detection uses a longer timeout when this is `false`.
    fn can_query_button1(&self) -> bool { true }
}

/// Detect the display backend at runtime and return the matching `Platform`.
///
/// * `WAYLAND_DISPLAY` set → `WaylandPlatform`  (paste via portal, no cursor tracking)
/// * Otherwise            → `X11Platform`        (paste via XTest, cursor via x11rb)
pub fn detect() -> std::sync::Arc<dyn Platform> {
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        eprintln!("[platform] Wayland session — using Wayland backend");
        std::sync::Arc::new(WaylandPlatform::new())
    } else {
        eprintln!("[platform] X11 session — using X11 backend");
        std::sync::Arc::new(X11Platform)
    }
}
