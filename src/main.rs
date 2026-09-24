mod app;
mod clipboard;
mod config;
mod controller;
mod events;
mod hotkey;
mod notify;
mod paths;
mod platform;
mod store;
mod tray;
mod ui;

use app::App;

fn main() {
    // ── Handle subcommands ───────────────────────────────────────────────────
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("reload") {
        reload_daemon();
        return;
    }

    // ── Wayland guard (user-facing error, shown before daemonizing) ──────────
    if std::env::var("WAYLAND_DISPLAY").is_ok()
        && std::env::var("GDK_BACKEND").as_deref() != Ok("x11")
    {
        eprintln!("╔══════════════════════════════════════════════════════════════════╗");
        eprintln!("║        Clipboard Manager requires an X11 session.               ║");
        eprintln!("╠══════════════════════════════════════════════════════════════════╣");
        eprintln!("║  Wayland is detected but is not fully supported.                ║");
        eprintln!("║                                                                  ║");
        eprintln!("║  To use Clipboard Manager, log out and at the login screen      ║");
        eprintln!("║  click the gear ⚙  icon and select  \"Ubuntu on Xorg\".          ║");
        eprintln!("║                                                                  ║");
        eprintln!("║  Advanced: to force X11 mode under Wayland (XWayland):          ║");
        eprintln!("║    GDK_BACKEND=x11 clipboard-manager                            ║");
        eprintln!("╚══════════════════════════════════════════════════════════════════╝");
        std::process::exit(1);
    }

    // ── Auto-daemonize (detach from terminal) ────────────────────────────────
    daemonize_if_needed();

    if let Err(e) = App::new().and_then(|a| a.run()) {
        eprintln!("Error: {:#}", e);
        std::process::exit(1);
    }
}

/// Kill the running daemon and start a fresh one.
/// Safe to run from a terminal: uses a pattern that matches the bare daemon
/// process (`clipboard-manager` with no args) but not this reload process
/// (`clipboard-manager reload`).
fn reload_daemon() {
    // `clipboard-manager$` matches cmdlines ending with the binary name only —
    // the reload process cmdline ends with "reload", so it is not killed.
    let killed = std::process::Command::new("pkill")
        .args(["-TERM", "-f", "clipboard-manager$"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if killed {
        // Give the old instance time to clean up.
        std::thread::sleep(std::time::Duration::from_millis(400));
    }

    let Ok(exe) = std::env::current_exe() else {
        eprintln!("clipboard-manager: reload failed (cannot locate executable)");
        std::process::exit(1);
    };

    match spawn_daemon(&exe) {
        Ok(_)  => println!("clipboard-manager: reloaded"),
        Err(e) => {
            eprintln!("clipboard-manager: reload failed: {e}");
            std::process::exit(1);
        }
    }
}

/// Re-exec the process detached from the terminal (stdin/stdout to
/// `/dev/null`, stderr to the log file).  The parent exits immediately; the child is
/// adopted by init and runs as a background daemon.
///
/// Skipped when:
/// * `_CM_DAEMON=1`  — we are already the daemon child
/// * `RUST_LOG` set  — user wants visible log output (foreground mode)
fn daemonize_if_needed() {
    if std::env::var("_CM_DAEMON").is_ok() || std::env::var("RUST_LOG").is_ok() {
        return;
    }

    let Ok(exe) = std::env::current_exe() else { return };

    if spawn_daemon(&exe).is_ok() {
        std::process::exit(0);
    }
    // Spawn failed → fall through and run in foreground as a graceful fallback.
}

/// Start the detached daemon child. Its stderr (where the log goes) is
/// written to `$XDG_STATE_HOME/clipboard-manager/clipboard-manager.log`,
/// truncated on every start, so problems are diagnosable after the fact.
fn spawn_daemon(exe: &std::path::Path) -> std::io::Result<std::process::Child> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::process::Stdio;
    let stderr = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(paths::state_dir().join("clipboard-manager.log"))
        .map(Stdio::from)
        .unwrap_or_else(|_| Stdio::null());
    std::process::Command::new(exe)
        .env("_CM_DAEMON", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .spawn()
}
