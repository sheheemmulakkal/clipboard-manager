use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, GrabMode, ModMask};
use x11rb::rust_connection::RustConnection;

use super::HotkeyManager;

// ── internal state kept alive while the grab is active ────────────────────────

struct ActiveGrab {
    stop:      Arc<AtomicBool>,
    modifiers: u16,
    keycode:   u8,
    #[allow(dead_code)]
    root:      u32,
}

// ── public struct ──────────────────────────────────────────────────────────────

pub struct X11HotkeyManager {
    hotkey: String,
    grab:   Mutex<Option<ActiveGrab>>,
}

impl X11HotkeyManager {
    pub fn new(hotkey: &str) -> Self {
        Self {
            hotkey: hotkey.to_string(),
            grab:   Mutex::new(None),
        }
    }
}

// ── HotkeyManager impl ────────────────────────────────────────────────────────

impl HotkeyManager for X11HotkeyManager {
    fn start(&self, on_hotkey: impl Fn() + Send + Sync + 'static) -> Result<()> {
        let (conn, screen_num) =
            RustConnection::connect(None).map_err(|e| anyhow!("X11 connect: {}", e))?;

        let setup    = conn.setup();
        let root     = setup.roots[screen_num].root;
        let first_kc = setup.min_keycode;
        let kc_count = setup.max_keycode - first_kc + 1;

        let km = conn
            .get_keyboard_mapping(first_kc, kc_count)
            .map_err(|e| anyhow!("{}", e))?
            .reply()
            .map_err(|e| anyhow!("{}", e))?;

        let (modifiers, keycode) = parse_hotkey(&self.hotkey, first_kc, &km)?;
        eprintln!("[hotkey] grabbing '{}' → modifiers=0x{:02x} keycode={}", self.hotkey, modifiers, keycode);

        // Grab with NumLock / CapsLock permutations so the hotkey fires
        // regardless of those lock-key states.
        let base = ModMask::from(modifiers);
        for mods in [
            base,
            base | ModMask::M2,
            base | ModMask::LOCK,
            base | ModMask::M2 | ModMask::LOCK,
        ] {
            match conn.grab_key(false, root, mods, keycode, GrabMode::ASYNC, GrabMode::ASYNC) {
                Ok(cookie) => {
                    if let Err(e) = cookie.check() {
                        eprintln!("[hotkey] grab_key rejected for mods=0x{:x}: {}", u16::from(mods), e);
                    }
                }
                Err(e) => eprintln!("[hotkey] grab_key send error for mods=0x{:x}: {}", u16::from(mods), e),
            }
        }
        conn.flush().map_err(|e| anyhow!("{}", e))?;
        eprintln!("[hotkey] grab registered, listening for KeyPress events");

        let stop       = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);

        *self.grab.lock().unwrap() = Some(ActiveGrab { stop, modifiers, keycode, root });

        // Background thread: poll for KeyPress events.
        // on_hotkey is Fn() + Send + 'static, safe to call from any thread.
        let on_hotkey = Arc::new(on_hotkey);
        std::thread::spawn(move || {
            loop {
                if stop_clone.load(Ordering::Relaxed) {
                    break;
                }
                match conn.poll_for_event() {
                    Ok(Some(x11rb::protocol::Event::KeyPress(_))) => {
                        eprintln!("[hotkey] KeyPress received");
                        on_hotkey();
                    }
                    Ok(None) => {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        });

        Ok(())
    }

    fn stop(&self) {
        let Some(g) = self.grab.lock().unwrap().take() else { return };

        g.stop.store(true, Ordering::Relaxed);

        // Ungrab via a fresh connection (the event connection lives in the thread).
        if let Ok((conn, screen_num)) = RustConnection::connect(None) {
            let root = conn.setup().roots[screen_num].root;
            let base = ModMask::from(g.modifiers);
            for mods in [
                base,
                base | ModMask::M2,
                base | ModMask::LOCK,
                base | ModMask::M2 | ModMask::LOCK,
            ] {
                let _ = conn.ungrab_key(g.keycode, root, mods);
            }
            let _ = conn.flush();
        }
    }
}

impl Drop for X11HotkeyManager {
    fn drop(&mut self) {
        self.stop();
    }
}

// ── hotkey string parser ───────────────────────────────────────────────────────

/// Parse `"ctrl+alt+c"` → `(modifier_mask: u16, keycode: u8)`.
/// Supported modifiers: ctrl/control, alt, super/win, shift.
/// Key must be a single ASCII letter (a-z).
fn parse_hotkey(
    hotkey: &str,
    first_kc: u8,
    km: &x11rb::protocol::xproto::GetKeyboardMappingReply,
) -> Result<(u16, u8)> {
    let mut modifier_mask: u16 = 0;
    let mut key_char: Option<char> = None;

    for part in hotkey.split('+') {
        match part.trim().to_lowercase().as_str() {
            "ctrl" | "control" => modifier_mask |= 0x04, // ControlMask
            "alt"              => modifier_mask |= 0x08, // Mod1Mask
            "super" | "win"    => modifier_mask |= 0x40, // Mod4Mask
            "shift"            => modifier_mask |= 0x01, // ShiftMask
            s if s.len() == 1  => key_char = s.chars().next(),
            other              => return Err(anyhow!("Unknown hotkey token: {}", other)),
        }
    }

    let key = key_char.ok_or_else(|| anyhow!("No key letter in hotkey: {}", hotkey))?;
    // For a–z the X11 keysym equals the ASCII value of the lowercase letter.
    let keysym = key.to_lowercase().next().unwrap() as u32;

    let kpk = km.keysyms_per_keycode as usize;
    let total_kcs = (km.keysyms.len() / kpk) as u8;

    let keycode = (first_kc..first_kc.saturating_add(total_kcs))
        .find(|&kc| {
            let idx = (kc - first_kc) as usize * kpk;
            km.keysyms.get(idx).copied().unwrap_or(0) == keysym
        })
        .ok_or_else(|| anyhow!("Keycode not found for '{}'", key))?;

    Ok((modifier_mask, keycode))
}
