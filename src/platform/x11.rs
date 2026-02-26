use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageData, ClientMessageEvent, ConnectionExt as _, EventMask,
    InputFocus, CLIENT_MESSAGE_EVENT, KEY_PRESS_EVENT, KEY_RELEASE_EVENT,
};
use x11rb::rust_connection::RustConnection;

use super::Platform;

/// X11 backend.  Uses `x11rb` for all platform operations and `gdk4-x11`
/// to obtain the GTK4 window's X11 window ID for repositioning.
pub struct X11Platform;

impl Platform for X11Platform {
    // ── capture_active_window ─────────────────────────────────────────────

    fn capture_active_window(&self) -> Option<u64> {
        let (conn, sn) = RustConnection::connect(None).ok()?;
        let root = conn.setup().roots[sn].root;

        let atom = conn
            .intern_atom(false, b"_NET_ACTIVE_WINDOW")
            .ok()?
            .reply()
            .ok()?
            .atom;

        let reply = conn
            .get_property(false, root, atom, AtomEnum::WINDOW, 0, 1)
            .ok()?
            .reply()
            .ok()?;

        let win = reply.value32()?.next()? as u64;
        if win == 0 { None } else { Some(win) }
    }

    // ── paste ─────────────────────────────────────────────────────────────

    fn paste(&self, prev_window: Option<u64>) {
        // Called from a background std::thread — blocking is fine.
        if let Err(e) = paste_xtest(prev_window, false) {
            eprintln!("[x11 paste] {e}");
        }
    }

    fn paste_terminal(&self, prev_window: Option<u64>) {
        if let Err(e) = paste_xtest(prev_window, true) {
            eprintln!("[x11 paste_terminal] {e}");
        }
    }

    // ── cursor_position ───────────────────────────────────────────────────

    fn cursor_position(&self) -> Option<(i32, i32)> {
        let (conn, sn) = RustConnection::connect(None).ok()?;
        let root = conn.setup().roots[sn].root;
        let r = conn.query_pointer(root).ok()?.reply().ok()?;
        Some((r.root_x as i32, r.root_y as i32))
    }

    // ── move_popup ────────────────────────────────────────────────────────

    fn move_popup(&self, window: &gtk4::Window, x: i32, y: i32) {
        use glib::object::Cast;
        use gtk4::prelude::NativeExt;

        let Some(surface) = window.surface() else { return };

        // Downcast GDK surface → X11Surface to obtain the XID.
        if let Ok(x11_surface) = surface.downcast::<gdk4_x11::X11Surface>() {
            let xid = x11_surface.xid() as u32;
            let Ok((conn, _)) = RustConnection::connect(None) else { return };
            let aux = x11rb::protocol::xproto::ConfigureWindowAux::new().x(x).y(y);
            let _ = conn.configure_window(xid, &aux);
            let _ = conn.flush();
        }
    }

    // ── button1_held ──────────────────────────────────────────────────────

    fn button1_held(&self) -> bool {
        use x11rb::protocol::xproto::KeyButMask;
        let Ok((conn, sn)) = RustConnection::connect(None) else { return false };
        let root = conn.setup().roots[sn].root;
        let cookie = match conn.query_pointer(root) {
            Ok(c)  => c,
            Err(_) => return false,
        };
        let reply = match cookie.reply() {
            Ok(r)  => r,
            Err(_) => return false,
        };
        reply.mask.contains(KeyButMask::BUTTON1)
    }
}

// ── screen_dimensions (used by popup.rs for coordinate clamping) ──────────────

/// Return the primary screen dimensions in pixels.
pub fn screen_dimensions() -> Option<(i32, i32)> {
    let (conn, sn) = RustConnection::connect(None).ok()?;
    let screen = &conn.setup().roots[sn];
    Some((screen.width_in_pixels as i32, screen.height_in_pixels as i32))
}

// ── paste implementation ──────────────────────────────────────────────────────

fn paste_xtest(prev_window: Option<u64>, use_shift: bool) -> Result<()> {
    let (conn, sn) = RustConnection::connect(None)
        .map_err(|e| anyhow!("X11 connect: {e}"))?;
    let root = conn.setup().roots[sn].root;

    // If we have a target window, activate it via EWMH before pasting.
    if let Some(win_id) = prev_window {
        activate_window(&conn, root, win_id as u32)?;
        std::thread::sleep(Duration::from_millis(100));
    }

    let ctrl = find_keycode(&conn, 0xffe3).context("Control_L keycode not found")?;
    let v    = find_keycode(&conn, 0x0076).context("'v' keycode not found")?;

    use x11rb::protocol::xtest::ConnectionExt as _;
    if use_shift {
        // Send Ctrl+Shift+V via XTest (terminal paste).
        let shift = find_keycode(&conn, 0xffe1).context("Shift_L keycode not found")?;
        conn.xtest_fake_input(KEY_PRESS_EVENT,   ctrl,  0, root, 0, 0, 0)?.check()?;
        conn.xtest_fake_input(KEY_PRESS_EVENT,   shift, 0, root, 0, 0, 0)?.check()?;
        conn.xtest_fake_input(KEY_PRESS_EVENT,   v,     0, root, 0, 0, 0)?.check()?;
        conn.xtest_fake_input(KEY_RELEASE_EVENT, v,     0, root, 0, 0, 0)?.check()?;
        conn.xtest_fake_input(KEY_RELEASE_EVENT, shift, 0, root, 0, 0, 0)?.check()?;
        conn.xtest_fake_input(KEY_RELEASE_EVENT, ctrl,  0, root, 0, 0, 0)?.check()?;
    } else {
        // Send Ctrl+V via XTest.
        conn.xtest_fake_input(KEY_PRESS_EVENT,   ctrl, 0, root, 0, 0, 0)?.check()?;
        conn.xtest_fake_input(KEY_PRESS_EVENT,   v,    0, root, 0, 0, 0)?.check()?;
        conn.xtest_fake_input(KEY_RELEASE_EVENT, v,    0, root, 0, 0, 0)?.check()?;
        conn.xtest_fake_input(KEY_RELEASE_EVENT, ctrl, 0, root, 0, 0, 0)?.check()?;
    }
    conn.flush().map_err(|e| anyhow!("flush: {e}"))?;

    Ok(())
}

/// Send an EWMH `_NET_ACTIVE_WINDOW` ClientMessage to ask the WM to activate
/// `win_id`.  More reliable than `set_input_focus` on modern compositors.
fn activate_window(conn: &RustConnection, root: u32, win_id: u32) -> Result<()> {
    let atom = conn
        .intern_atom(false, b"_NET_ACTIVE_WINDOW")
        .map_err(|e| anyhow!("{e}"))?
        .reply()
        .map_err(|e| anyhow!("{e}"))?
        .atom;

    let event = ClientMessageEvent {
        response_type: CLIENT_MESSAGE_EVENT,
        format:        32,
        sequence:      0,
        window:        win_id,
        type_:         atom,
        // [source_indication=2 (pager), timestamp=0, active_window=0, 0, 0]
        data: ClientMessageData::from([2u32, 0, 0, 0, 0]),
    };

    conn.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT,
        event,
    )
    .map_err(|e| anyhow!("{e}"))?;

    // Also call set_input_focus as a fallback for WMs that don't honour EWMH.
    let _ = conn.set_input_focus(InputFocus::POINTER_ROOT, win_id, 0u32);

    conn.flush().map_err(|e| anyhow!("{e}"))?;
    Ok(())
}

/// Look up the X11 keycode for a given keysym.
fn find_keycode(conn: &RustConnection, keysym: u32) -> Option<u8> {
    let setup    = conn.setup();
    let first_kc = setup.min_keycode;
    let kc_count = setup.max_keycode - first_kc + 1;
    let km = conn.get_keyboard_mapping(first_kc, kc_count).ok()?.reply().ok()?;
    let kpk = km.keysyms_per_keycode as usize;
    (first_kc..=setup.max_keycode).find(|&kc| {
        let idx = (kc - first_kc) as usize * kpk;
        km.keysyms.get(idx).copied().unwrap_or(0) == keysym
    })
}
