use tokio::runtime::Runtime;
use tokio::sync::{mpsc, oneshot};

use super::Platform;

/// Wayland backend.
///
/// * Paste: `org.freedesktop.portal.RemoteDesktop` (ashpd). A saved restore
///   token means the permission dialog appears once, not on every login.
/// * Cursor: not exposed by Wayland; returns `None` (the popup is centred).
/// * `move_popup`: no-op; the compositor positions windows.
/// * `button1_held` / `can_query_button1`: always false; Wayland does not
///   expose pointer button state to other clients.
pub struct WaylandPlatform {
    #[allow(dead_code)] // keeps the runtime alive for the paste daemon task
    rt:       Runtime,
    paste_tx: mpsc::Sender<(bool, oneshot::Sender<()>)>,
}

impl WaylandPlatform {
    pub fn new() -> Self {
        let rt = Runtime::new().expect("tokio Runtime");
        let (paste_tx, paste_rx) = mpsc::channel::<(bool, oneshot::Sender<()>)>(4);
        rt.spawn(paste_session_daemon(paste_rx));
        Self { rt, paste_tx }
    }
}

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
        if self.paste_tx.blocking_send((false, done_tx)).is_err() {
            tracing::warn!("wayland paste: daemon unavailable");
            return;
        }
        // Wait for the daemon to confirm the keystrokes were sent.
        let _ = done_rx.blocking_recv();
    }

    fn paste_terminal(&self, _prev_window: Option<u64>) {
        let (done_tx, done_rx) = oneshot::channel::<()>();
        if self.paste_tx.blocking_send((true, done_tx)).is_err() {
            tracing::warn!("wayland paste_terminal: daemon unavailable");
            return;
        }
        let _ = done_rx.blocking_recv();
    }

    /// The UI runs on XWayland (see main.rs), where the X11 TARGETS query
    /// sees the compositor's mirrored clipboard, including the password hint.
    fn clipboard_targets(&self) -> Option<Vec<String>> {
        if std::env::var("GDK_BACKEND").as_deref() == Ok("x11") {
            super::x11::clipboard_targets_x11().ok()
        } else {
            None
        }
    }

    fn cursor_position(&self) -> Option<(i32, i32)> {
        None
    }

    fn move_popup(&self, _window: &gtk4::Window, _x: i32, _y: i32) {}

    fn button1_held(&self) -> bool { false }

    fn can_query_button1(&self) -> bool { false }
}

// ── RemoteDesktop paste daemon ────────────────────────────────────────────────
//
// Runs as a tokio task for the lifetime of the app.
//
// * The session is opened on the first paste request (so the permission
//   dialog only appears when the user actually pastes), with a restore token
//   saved from an earlier grant — then no dialog appears at all.
// * The session is closed after `IDLE_CLOSE` without pastes: GNOME shows a
//   "remote control" indicator in the top bar while a session is open.
// * Each successful start returns a fresh restore token, which is saved.
//
// If the portal fails, the paste is a no-op (the item is still on the
// clipboard) and the next paste tries again.

/// Close the RemoteDesktop session after this long without pastes.
const IDLE_CLOSE: std::time::Duration = std::time::Duration::from_secs(10);

type PasteRequest = (bool, oneshot::Sender<()>);

async fn paste_session_daemon(mut rx: mpsc::Receiver<PasteRequest>) {
    use ashpd::desktop::remote_desktop::RemoteDesktop;

    let proxy = match RemoteDesktop::new().await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("wayland paste: RemoteDesktop portal unavailable: {e}");
            drain(rx).await;
            return;
        }
    };

    while let Some((shift, done)) = rx.recv().await {
        let Some(session) = open_session(&proxy).await else {
            let _ = done.send(());
            continue;
        };
        send_paste(&proxy, &session, shift).await;
        let _ = done.send(());

        // Serve further pastes until the session has been idle for a while.
        loop {
            match tokio::time::timeout(IDLE_CLOSE, rx.recv()).await {
                Ok(Some((shift, done))) => {
                    send_paste(&proxy, &session, shift).await;
                    let _ = done.send(());
                }
                Ok(None) => {
                    let _ = session.close().await;
                    return;
                }
                Err(_idle) => {
                    let _ = session.close().await;
                    tracing::debug!("wayland paste: session closed (idle)");
                    break;
                }
            }
        }
    }
}

/// Create and start a keyboard RemoteDesktop session.
async fn open_session<'a>(
    proxy: &'a ashpd::desktop::remote_desktop::RemoteDesktop<'a>,
) -> Option<ashpd::desktop::Session<'a, ashpd::desktop::remote_desktop::RemoteDesktop<'a>>> {
    use ashpd::desktop::remote_desktop::DeviceType;
    use ashpd::desktop::PersistMode;
    use ashpd::WindowIdentifier;

    let session = proxy
        .create_session()
        .await
        .map_err(|e| tracing::warn!("wayland paste: create_session failed: {e}"))
        .ok()?;

    // A token from an earlier grant skips the permission dialog.
    let token_path = crate::paths::state_dir().join("portal-restore-token");
    let saved_token = load_token(&token_path);
    proxy
        .select_devices(
            &session,
            DeviceType::Keyboard.into(),
            saved_token.as_deref(),
            PersistMode::ExplicitlyRevoked, // remember grant across re-launches
        )
        .await
        .map_err(|e| tracing::warn!("wayland paste: select_devices failed: {e}"))
        .ok()?;

    // Without a valid token this shows GNOME's "Allow remote interaction?" dialog.
    let selected = proxy
        .start(&session, &WindowIdentifier::default())
        .await
        .and_then(|r| r.response())
        .map_err(|e| tracing::warn!("wayland paste: start failed: {e}"))
        .ok()?;
    if let Some(token) = selected.restore_token() {
        save_token(&token_path, token);
    }
    tracing::debug!("wayland paste: RemoteDesktop session ready");
    Some(session)
}

async fn send_paste<'a>(
    proxy:   &ashpd::desktop::remote_desktop::RemoteDesktop<'a>,
    session: &ashpd::desktop::Session<'a, ashpd::desktop::remote_desktop::RemoteDesktop<'a>>,
    terminal: bool,
) {
    if terminal {
        send_ctrl_shift_v(proxy, session).await;
    } else {
        send_ctrl_v(proxy, session).await;
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

/// Send Ctrl+Shift+V via the open RemoteDesktop session (terminal paste).
async fn send_ctrl_shift_v<'a>(
    proxy:   &ashpd::desktop::remote_desktop::RemoteDesktop<'a>,
    session: &ashpd::desktop::Session<'a, ashpd::desktop::remote_desktop::RemoteDesktop<'a>>,
) {
    use ashpd::desktop::remote_desktop::KeyState;
    // Control_L = 0xffe3, Shift_L = 0xffe1, 'v' = 0x76
    let _ = proxy.notify_keyboard_keysym(session, 0xffe3, KeyState::Pressed).await;
    let _ = proxy.notify_keyboard_keysym(session, 0xffe1, KeyState::Pressed).await;
    let _ = proxy.notify_keyboard_keysym(session, 0x0076, KeyState::Pressed).await;
    let _ = proxy.notify_keyboard_keysym(session, 0x0076, KeyState::Released).await;
    let _ = proxy.notify_keyboard_keysym(session, 0xffe1, KeyState::Released).await;
    let _ = proxy.notify_keyboard_keysym(session, 0xffe3, KeyState::Released).await;
}

/// Read a saved RemoteDesktop restore token.
fn load_token(path: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Save a RemoteDesktop restore token (0600: it grants input injection).
fn save_token(path: &std::path::Path, token: &str) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let result = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .and_then(|mut f| f.write_all(token.as_bytes()));
    if let Err(e) = result {
        tracing::warn!("wayland paste: cannot save restore token: {e}");
    }
}

/// Drain remaining paste requests after a fatal error (so callers unblock).
async fn drain(mut rx: mpsc::Receiver<PasteRequest>) {
    while let Some((_, tx)) = rx.recv().await {
        let _ = tx.send(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_token_roundtrip() {
        let dir = std::env::temp_dir().join(format!("cm-test-{}-token", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("portal-restore-token");
        assert_eq!(load_token(&path), None);
        save_token(&path, "abc-123");
        assert_eq!(load_token(&path).as_deref(), Some("abc-123"));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }
}
