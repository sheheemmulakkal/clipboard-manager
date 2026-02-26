use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use evdev::{Device, EventType, Key};

/// Start a global hotkey listener using raw evdev input events.
///
/// This is the Wayland fallback when the `org.freedesktop.portal.GlobalShortcuts`
/// portal is unavailable (compositors older than GNOME 45 / KDE Plasma 6).
///
/// Requires the user to be in the `input` group:
///   sudo usermod -aG input $USER   (then log out and back in)
///
/// Spawns one background thread per accessible keyboard device.
/// Returns `true` if at least one keyboard listener was started, `false` otherwise.
pub fn start(hotkey: &str, cb: Arc<dyn Fn() + Send + Sync + 'static>) -> bool {
    let parsed = match parse_hotkey(hotkey) {
        Some(p) => p,
        None => {
            tracing::warn!("hotkey/evdev: cannot parse hotkey string: '{hotkey}'");
            return false;
        }
    };

    let keyboards = find_keyboards();
    if keyboards.is_empty() {
        tracing::warn!("hotkey/evdev: no readable keyboard devices (not in 'input' group)");
        return false;
    }

    tracing::info!(
        "hotkey/evdev: listening on {} keyboard device(s) for '{hotkey}'",
        keyboards.len(),
    );

    // Shared pressed-key state (by evdev keycode) across all device threads.
    let pressed: Arc<Mutex<HashSet<u16>>> = Arc::new(Mutex::new(HashSet::new()));

    // Convert modifier groups to raw code pairs for fast comparison.
    let modifier_groups: Arc<Vec<(u16, u16)>> = Arc::new(
        parsed
            .modifier_groups
            .iter()
            .map(|(l, r)| (l.code(), r.code()))
            .collect(),
    );
    let trigger_code = parsed.trigger.code();

    for mut device in keyboards.into_iter() {
        let pressed         = Arc::clone(&pressed);
        let modifier_groups = Arc::clone(&modifier_groups);
        let cb              = Arc::clone(&cb);

        std::thread::Builder::new()
            .name("evdev-kbd".into())
            .spawn(move || {
                loop {
                    let events = match device.fetch_events() {
                        Ok(ev)  => ev,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(std::time::Duration::from_millis(10));
                            continue;
                        }
                        Err(e) => {
                            tracing::warn!("hotkey/evdev: device read error: {e}");
                            break;
                        }
                    };

                    for event in events {
                        if event.event_type() != EventType::KEY {
                            continue;
                        }
                        let code  = event.code();
                        let value = event.value();

                        let mut state = pressed.lock().unwrap();
                        match value {
                            0 => {
                                // key release
                                state.remove(&code);
                            }
                            1 => {
                                // key press (not repeat)
                                state.insert(code);

                                if code == trigger_code {
                                    let all_mods = modifier_groups
                                        .iter()
                                        .all(|(l, r)| state.contains(l) || state.contains(r));

                                    if all_mods {
                                        drop(state); // release lock before calling back
                                        cb();
                                        break; // break inner for-loop; outer loop resumes
                                    }
                                }
                            }
                            _ => {} // value 2 = key repeat, ignored
                        }
                    }
                }
            })
            .ok();
    }

    true
}

// ── Internal types ────────────────────────────────────────────────────────────

struct ParsedHotkey {
    /// Each entry is (left-variant, right-variant) for one logical modifier.
    modifier_groups: Vec<(Key, Key)>,
    trigger: Key,
}

fn parse_hotkey(s: &str) -> Option<ParsedHotkey> {
    let mut groups  = Vec::new();
    let mut trigger = None;

    for part in s.split('+') {
        match part.trim().to_lowercase().as_str() {
            "ctrl" | "control"       => groups.push((Key::KEY_LEFTCTRL,  Key::KEY_RIGHTCTRL)),
            "alt"                    => groups.push((Key::KEY_LEFTALT,   Key::KEY_RIGHTALT)),
            "super" | "win" | "meta" => groups.push((Key::KEY_LEFTMETA, Key::KEY_RIGHTMETA)),
            "shift"                  => groups.push((Key::KEY_LEFTSHIFT, Key::KEY_RIGHTSHIFT)),
            k                        => trigger = str_to_key(k),
        }
    }

    trigger.map(|t| ParsedHotkey { modifier_groups: groups, trigger: t })
}

fn str_to_key(s: &str) -> Option<Key> {
    // Single character → letter or digit
    if s.len() == 1 {
        let c = s.chars().next().unwrap();
        return char_to_key(c);
    }
    match s {
        "space"            => Some(Key::KEY_SPACE),
        "enter" | "return" => Some(Key::KEY_ENTER),
        "tab"              => Some(Key::KEY_TAB),
        "escape" | "esc"   => Some(Key::KEY_ESC),
        "backspace"        => Some(Key::KEY_BACKSPACE),
        "delete" | "del"   => Some(Key::KEY_DELETE),
        "insert" | "ins"   => Some(Key::KEY_INSERT),
        "home"             => Some(Key::KEY_HOME),
        "end"              => Some(Key::KEY_END),
        "pageup"           => Some(Key::KEY_PAGEUP),
        "pagedown"         => Some(Key::KEY_PAGEDOWN),
        "f1"               => Some(Key::KEY_F1),
        "f2"               => Some(Key::KEY_F2),
        "f3"               => Some(Key::KEY_F3),
        "f4"               => Some(Key::KEY_F4),
        "f5"               => Some(Key::KEY_F5),
        "f6"               => Some(Key::KEY_F6),
        "f7"               => Some(Key::KEY_F7),
        "f8"               => Some(Key::KEY_F8),
        "f9"               => Some(Key::KEY_F9),
        "f10"              => Some(Key::KEY_F10),
        "f11"              => Some(Key::KEY_F11),
        "f12"              => Some(Key::KEY_F12),
        _                  => None,
    }
}

fn char_to_key(c: char) -> Option<Key> {
    match c.to_ascii_lowercase() {
        'a' => Some(Key::KEY_A), 'b' => Some(Key::KEY_B),
        'c' => Some(Key::KEY_C), 'd' => Some(Key::KEY_D),
        'e' => Some(Key::KEY_E), 'f' => Some(Key::KEY_F),
        'g' => Some(Key::KEY_G), 'h' => Some(Key::KEY_H),
        'i' => Some(Key::KEY_I), 'j' => Some(Key::KEY_J),
        'k' => Some(Key::KEY_K), 'l' => Some(Key::KEY_L),
        'm' => Some(Key::KEY_M), 'n' => Some(Key::KEY_N),
        'o' => Some(Key::KEY_O), 'p' => Some(Key::KEY_P),
        'q' => Some(Key::KEY_Q), 'r' => Some(Key::KEY_R),
        's' => Some(Key::KEY_S), 't' => Some(Key::KEY_T),
        'u' => Some(Key::KEY_U), 'v' => Some(Key::KEY_V),
        'w' => Some(Key::KEY_W), 'x' => Some(Key::KEY_X),
        'y' => Some(Key::KEY_Y), 'z' => Some(Key::KEY_Z),
        '0' => Some(Key::KEY_0), '1' => Some(Key::KEY_1),
        '2' => Some(Key::KEY_2), '3' => Some(Key::KEY_3),
        '4' => Some(Key::KEY_4), '5' => Some(Key::KEY_5),
        '6' => Some(Key::KEY_6), '7' => Some(Key::KEY_7),
        '8' => Some(Key::KEY_8), '9' => Some(Key::KEY_9),
        _   => None,
    }
}

/// Return all keyboard devices that can be opened.
/// Devices that require elevated permissions are silently skipped by evdev.
fn find_keyboards() -> Vec<Device> {
    evdev::enumerate()
        .filter_map(|(_path, device)| {
            let keys = device.supported_keys()?;
            // A real keyboard should expose at least the letter keys.
            if keys.contains(Key::KEY_A) && keys.contains(Key::KEY_Z) {
                Some(device)
            } else {
                None
            }
        })
        .collect()
}
