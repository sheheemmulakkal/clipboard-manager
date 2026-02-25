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
    fn start(&self, on_hotkey: Box<dyn Fn() + Send + Sync + 'static>) -> Result<()> {
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

/// Parse `"ctrl+alt+v"` → `(modifier_mask: u16, keycode: u8)`.
///
/// Modifiers (case-insensitive): ctrl/control, alt, super/win, shift.
/// Keys: any single letter a–z, or named keys:
///   space, return, escape, tab, backspace, f1–f12.
fn parse_hotkey(
    hotkey: &str,
    first_kc: u8,
    km: &x11rb::protocol::xproto::GetKeyboardMappingReply,
) -> Result<(u16, u8)> {
    let mut modifier_mask: u16 = 0;
    let mut key_keysym: Option<u32> = None;

    for part in hotkey.split('+') {
        let token = part.trim().to_lowercase();
        match token.as_str() {
            "ctrl" | "control" => modifier_mask |= 0x04, // ControlMask
            "alt"              => modifier_mask |= 0x08, // Mod1Mask
            "super" | "win"    => modifier_mask |= 0x40, // Mod4Mask
            "shift"            => modifier_mask |= 0x01, // ShiftMask
            s => {
                let sym = keysym_for_name(s).ok_or_else(|| {
                    anyhow!("Invalid hotkey '{}': unknown key '{}'", hotkey, s)
                })?;
                key_keysym = Some(sym);
            }
        }
    }

    let keysym = key_keysym
        .ok_or_else(|| anyhow!("Invalid hotkey '{}': no key specified", hotkey))?;

    let kpk = km.keysyms_per_keycode as usize;
    let total_kcs = (km.keysyms.len() / kpk) as u8;

    let keycode = (first_kc..first_kc.saturating_add(total_kcs))
        .find(|&kc| {
            let idx = (kc - first_kc) as usize * kpk;
            km.keysyms.get(idx).copied().unwrap_or(0) == keysym
        })
        .ok_or_else(|| {
            anyhow!("Invalid hotkey '{}': key not found on this keyboard", hotkey)
        })?;

    Ok((modifier_mask, keycode))
}

/// Map a key name to its X11 keysym value.
///
/// Letters: lowercase a–z → keysym == ASCII value (0x61–0x7a).
/// Named keys follow X11 keysym definitions.
fn keysym_for_name(name: &str) -> Option<u32> {
    // Single letter a-z
    if name.len() == 1 {
        let c = name.chars().next()?;
        if c.is_ascii_alphabetic() {
            return Some(c.to_ascii_lowercase() as u32);
        }
        // Single digit 0-9
        if c.is_ascii_digit() {
            return Some(c as u32);
        }
    }

    match name {
        "space"     => Some(0x0020),
        "return"    => Some(0xff0d),
        "escape"    => Some(0xff1b),
        "tab"       => Some(0xff09),
        "backspace" => Some(0xff08),
        "f1"        => Some(0xffbe),
        "f2"        => Some(0xffbf),
        "f3"        => Some(0xffc0),
        "f4"        => Some(0xffc1),
        "f5"        => Some(0xffc2),
        "f6"        => Some(0xffc3),
        "f7"        => Some(0xffc4),
        "f8"        => Some(0xffc5),
        "f9"        => Some(0xffc6),
        "f10"       => Some(0xffc7),
        "f11"       => Some(0xffc8),
        "f12"       => Some(0xffc9),
        _           => None,
    }
}
