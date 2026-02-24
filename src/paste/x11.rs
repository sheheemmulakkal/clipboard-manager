pub struct Paster;

impl Paster {
    /// Paste into a specific X11 window by ID.
    /// Strategy: focus the window, wait, then send ctrl+v to the focused window.
    /// Using `key --window` (XSendEvent) is avoided because many apps
    /// (terminals, browsers) ignore synthetic XSendEvent key events.
    pub fn paste_into_window(window_id: u64) -> anyhow::Result<()> {
        eprintln!("[paste] focusing window {window_id}");

        let out = std::process::Command::new("xdotool")
            .args(["windowfocus", "--sync", &window_id.to_string()])
            .output()?;

        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            eprintln!("[paste] windowfocus failed ({err}), falling back to focused paste");
            return Self::paste_into_focused();
        }

        // Give the WM time to complete the focus transfer.
        std::thread::sleep(std::time::Duration::from_millis(100));

        eprintln!("[paste] sending ctrl+v to focused window");
        let out = std::process::Command::new("xdotool")
            .args(["key", "--clearmodifiers", "ctrl+v"])
            .output()?;

        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            anyhow::bail!("xdotool key ctrl+v failed: {err}");
        }

        Ok(())
    }

    /// Fallback: paste into whatever window currently has focus.
    pub fn paste_into_focused() -> anyhow::Result<()> {
        eprintln!("[paste] pasting into focused window (fallback)");
        std::thread::sleep(std::time::Duration::from_millis(150));
        let out = std::process::Command::new("xdotool")
            .args(["key", "--clearmodifiers", "ctrl+v"])
            .output()?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            anyhow::bail!("xdotool key ctrl+v (focused) failed: {err}");
        }
        Ok(())
    }
}
