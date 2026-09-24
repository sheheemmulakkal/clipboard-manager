//! User-visible error reporting.
//!
//! The daemon runs detached from any terminal, so errors that the user must
//! act on (bad config, hotkey not registered) are shown as a desktop
//! notification in addition to being logged.

pub fn error(summary: &str, body: &str) {
    tracing::error!("{summary}: {body}");
    let shown = std::process::Command::new("notify-send")
        .args(["--app-name=Clipboard Manager", "--icon=edit-paste", summary, body])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !shown {
        tracing::warn!("notify-send unavailable — error only in the log");
    }
}
