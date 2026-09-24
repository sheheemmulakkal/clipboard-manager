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
            // ANY: WMs use type WINDOW, but tolerate CARDINAL.
            .get_property(false, root, atom, AtomEnum::ANY, 0, 1)
            .ok()?
            .reply()
            .ok()?;

        let win = reply.value32()?.next()? as u64;
        if win == 0 { None } else { Some(win) }
    }

    // ── active_window_class ───────────────────────────────────────────────

    fn active_window_class(&self) -> Option<Vec<String>> {
        let win = self.capture_active_window()? as u32;
        let (conn, _) = RustConnection::connect(None).ok()?;
        let reply = conn
            .get_property(false, win, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
            .ok()?
            .reply()
            .ok()?;
        // WM_CLASS = "instance\0class\0"
        let classes: Vec<String> = reply
            .value
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();
        (!classes.is_empty()).then_some(classes)
    }

    // ── clipboard_targets ─────────────────────────────────────────────────

    fn clipboard_targets(&self) -> Option<Vec<String>> {
        clipboard_targets_x11().ok()
    }

    // ── paste ─────────────────────────────────────────────────────────────

    fn paste(&self, prev_window: Option<u64>) {
        // Called from a background std::thread — blocking is fine.
        if let Err(e) = paste_xtest(prev_window, false) {
            tracing::warn!("x11 paste: {e}");
        }
    }

    fn paste_terminal(&self, prev_window: Option<u64>) {
        if let Err(e) = paste_xtest(prev_window, true) {
            tracing::warn!("x11 paste_terminal: {e}");
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

// ── clipboard TARGETS query ───────────────────────────────────────────────────

/// Ask the CLIPBOARD owner for its TARGETS list. Waits at most 200 ms for an
/// answer so a hung owner can't stall the caller.
fn clipboard_targets_x11() -> Result<Vec<String>> {
    use x11rb::protocol::xproto::{CreateWindowAux, WindowClass};
    use x11rb::protocol::Event;
    use x11rb::CURRENT_TIME;

    let (conn, sn) = RustConnection::connect(None).map_err(|e| anyhow!("X11 connect: {e}"))?;
    let root = conn.setup().roots[sn].root;
    let win  = conn.generate_id()?;
    conn.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT, win, root, 0, 0, 1, 1, 0,
        WindowClass::INPUT_ONLY, x11rb::COPY_FROM_PARENT, &CreateWindowAux::new(),
    )?;
    let atom = |name: &[u8]| -> Result<u32> { Ok(conn.intern_atom(false, name)?.reply()?.atom) };
    let clipboard = atom(b"CLIPBOARD")?;
    let targets   = atom(b"TARGETS")?;
    let property  = atom(b"CLIPBOARD_MANAGER_TARGETS")?;

    conn.convert_selection(win, clipboard, targets, property, CURRENT_TIME)?;
    conn.flush()?;

    let deadline = std::time::Instant::now() + Duration::from_millis(200);
    let answered = loop {
        match conn.poll_for_event()? {
            Some(Event::SelectionNotify(e)) if e.requestor == win => break e.property != 0,
            Some(_) => {}
            None if std::time::Instant::now() > deadline => break false,
            None => std::thread::sleep(Duration::from_millis(2)),
        }
    };
    if !answered {
        let _ = conn.destroy_window(win);
        return Err(anyhow!("no TARGETS reply"));
    }

    let reply = conn.get_property(true, win, property, AtomEnum::ANY, 0, 1024)?.reply()?;
    let atoms: Vec<u32> = reply.value32().map(|v| v.collect()).unwrap_or_default();
    let cookies: Vec<_> = atoms.iter().filter_map(|a| conn.get_atom_name(*a).ok()).collect();
    let names = cookies
        .into_iter()
        .filter_map(|c| c.reply().ok())
        .map(|r| String::from_utf8_lossy(&r.name).into_owned())
        .collect();
    let _ = conn.destroy_window(win);
    Ok(names)
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
