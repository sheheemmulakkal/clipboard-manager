use tokio::runtime::Runtime;
use tokio::sync::{mpsc, oneshot};

use super::Platform;

/// Wayland backend.
///
/// * Paste     – `org.freedesktop.portal.RemoteDesktop` (ashpd).
///               A single portal session is created on the first paste and
///               reused for every subsequent paste, so the permission dialog
///               appears at most once per application run.
/// * Cursor    – not exposed by Wayland; returns `None`.
/// * move_popup – no-op; the compositor positions windows.
/// * button1_held / can_query_button1 – always false; Wayland does not expose
///               pointer button state to other clients.
pub struct WaylandPlatform {
    #[allow(dead_code)] // keeps the runtime alive for the paste daemon task
    rt:       Runtime,
    paste_tx: mpsc::Sender<oneshot::Sender<()>>,
}

impl WaylandPlatform {
    pub fn new() -> Self {
        let rt = Runtime::new().expect("tokio Runtime");
        let (paste_tx, paste_rx) = mpsc::channel::<oneshot::Sender<()>>(4);
        rt.spawn(paste_session_daemon(paste_rx));
        Self { rt, paste_tx }
    }
}

// SAFETY: tokio Runtime and mpsc::Sender are Send+Sync.
unsafe impl Send for WaylandPlatform {}
unsafe impl Sync for WaylandPlatform {}

impl Platform for WaylandPlatform {
    fn capture_active_window(&self) -> Option<u64> {
        None
    }

    /// Paste clipboard contents via the persistent RemoteDesktop session.
    ///
    /// Blocks the calling thread until the paste is delivered (or fails).
    /// Called from a `std::thread::spawn` thread in app.rs — never from the
    /// GTK main thread or a tokio thread, so `blocking_send` / `blocking_recv`
    /// are safe here.
    fn paste(&self, _prev_window: Option<u64>) {
        let (done_tx, done_rx) = oneshot::channel::<()>();
        if self.paste_tx.blocking_send(done_tx).is_err() {
            eprintln!("[wayland paste] paste daemon unavailable");
            return;
        }
        // Wait for the daemon to confirm the keystrokes were sent.
        let _ = done_rx.blocking_recv();
    }

    fn cursor_position(&self) -> Option<(i32, i32)> {
        None
    }

    fn move_popup(&self, _window: &gtk4::Window, _x: i32, _y: i32) {}

    fn button1_held(&self) -> bool { false }

    fn can_query_button1(&self) -> bool { false }
}

// ── Persistent RemoteDesktop session daemon ───────────────────────────────────
//
// Runs as a tokio task for the lifetime of the app.
//
// Lifecycle:
//   1. Waits for the first paste request (so the permission dialog only
//      appears when the user actually tries to paste — not at app startup).
//   2. Creates a RemoteDesktop session and calls `start()`. On first run this
//      shows the "Allow remote interaction?" GNOME dialog. The user grants
//      permission once.
//   3. Serves every subsequent paste request by re-using the open session.
//      No further dialogs appear.
//
// If the session fails to initialize, paste requests are silently completed
// (the paste is a no-op but the app keeps running).

async fn paste_session_daemon(mut rx: mpsc::Receiver<oneshot::Sender<()>>) {
    use ashpd::desktop::remote_desktop::{DeviceType, RemoteDesktop};
    use ashpd::desktop::PersistMode;
    use ashpd::WindowIdentifier;

    // ── Wait for the first paste request ────────────────────────────────────
    let first_done = match rx.recv().await {
        Some(tx) => tx,
        None     => return,
    };

    // ── Create the session (shows dialog on first run) ───────────────────────
    let proxy = match RemoteDesktop::new().await {
        Ok(p)  => p,
        Err(e) => {
            eprintln!("[wayland paste] failed to connect to RemoteDesktop portal: {e}");
            let _ = first_done.send(());
            drain(rx).await;
            return;
        }
    };

    let session = match proxy.create_session().await {
        Ok(s)  => s,
        Err(e) => {
            eprintln!("[wayland paste] create_session failed: {e}");
            let _ = first_done.send(());
            drain(rx).await;
            return;
        }
    };

    if let Err(e) = proxy
        .select_devices(
            &session,
            DeviceType::Keyboard.into(),
            None,
            PersistMode::ExplicitlyRevoked, // remember grant across re-launches
        )
        .await
    {
        eprintln!("[wayland paste] select_devices failed: {e}");
        let _ = first_done.send(());
        drain(rx).await;
        return;
    }

    // `start()` triggers the one-time GNOME permission dialog.
    if let Err(e) = proxy.start(&session, &WindowIdentifier::default()).await {
        eprintln!("[wayland paste] start failed: {e}");
        let _ = first_done.send(());
        drain(rx).await;
        return;
    }

    eprintln!("[wayland paste] RemoteDesktop session ready — subsequent pastes need no dialog");

    // ── Serve the first paste, then all subsequent ones ──────────────────────
    send_ctrl_v(&proxy, &session).await;
    let _ = first_done.send(());

    while let Some(done_tx) = rx.recv().await {
        send_ctrl_v(&proxy, &session).await;
        let _ = done_tx.send(());
    }
}

/// Send Ctrl+V via the open RemoteDesktop session.
async fn send_ctrl_v<'a>(
    proxy:   &ashpd::desktop::remote_desktop::RemoteDesktop<'a>,
    session: &ashpd::desktop::Session<'a, ashpd::desktop::remote_desktop::RemoteDesktop<'a>>,
) {
    use ashpd::desktop::remote_desktop::KeyState;
    // Control_L keysym = 0xffe3,  'v' keysym = 0x76
    let _ = proxy.notify_keyboard_keysym(session, 0xffe3, KeyState::Pressed).await;
    let _ = proxy.notify_keyboard_keysym(session, 0x0076, KeyState::Pressed).await;
    let _ = proxy.notify_keyboard_keysym(session, 0x0076, KeyState::Released).await;
    let _ = proxy.notify_keyboard_keysym(session, 0xffe3, KeyState::Released).await;
}

/// Drain remaining paste requests after a fatal error (so callers unblock).
async fn drain(mut rx: mpsc::Receiver<oneshot::Sender<()>>) {
    while let Some(tx) = rx.recv().await {
        let _ = tx.send(());
    }
}
