mod app;
mod cli;
mod clipboard;
mod config;
mod controller;
mod crash;
mod events;
mod hotkey;
mod instance;
mod notify;
mod paths;
mod platform;
mod store;
mod tray;
mod ui;

use app::App;
use glib::prelude::ToVariant;

fn main() {
    crash::install_hook();
    let args: Vec<String> = std::env::args().collect();
    let command = match cli::parse(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("clipboard-manager: {e}");
            std::process::exit(2);
        }
    };
    match command {
        cli::Command::Help => {
            print!("{}", cli::USAGE);
            return;
        }
        cli::Command::Version => {
            println!("clipboard-manager {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        cli::Command::Reload => {
            reload_daemon();
            return;
        }
        cli::Command::List { limit } => {
            // Read-only: straight from the history file, daemon or not.
            let entries = store::engine::PersistenceEngine::new(paths::history_file()).load();
            print!("{}", cli::format_list(&ui::filter::sorted(entries), limit));
            return;
        }
        ref c if c.is_remote() => std::process::exit(send_to_running(&args, c)),
        _ => {}
    }

    // ── Wayland: run the UI on XWayland ──────────────────────────────────────
    // Native Wayland clients can only read the clipboard while focused; an
    // XWayland client can watch it in the background (the compositor
    // mirrors the clipboard to X11), and can place its own window.
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let xwayland = std::env::var_os("DISPLAY").is_some();
    let chosen = std::env::var("GDK_BACKEND").ok();
    if let Some(backend) = gdk_backend_for(wayland, xwayland, chosen.as_deref()) {
        // Single-threaded here: nothing else reads the environment yet.
        std::env::set_var("GDK_BACKEND", backend);
    } else if wayland && !xwayland {
        eprintln!("clipboard-manager: no XWayland — clipboard history only records while the popup is focused");
    }

    // ── Already running? Just open its popup. ────────────────────────────────
    // (Before loading history / image GC / truncating the log file, which
    // would race with the running instance.)
    if std::env::var_os("_CM_DAEMON").is_none() {
        if is_running() {
            let show = vec![args[0].clone(), "show".to_string()];
            std::process::exit(send_to_running(&show, &cli::Command::Show));
        }
        // Running, but not reachable over D-Bus from here (e.g. started
        // without a session bus): never start a second copy.
        if instance::is_locked(&instance::lock_path()) {
            eprintln!("clipboard-manager is already running");
            return;
        }
    }

    // ── Auto-daemonize (detach from terminal) ────────────────────────────────
    daemonize_if_needed();

    // ── Single instance (independent of D-Bus) ───────────────────────────────
    // Held until exit. A reload's new instance waits for the old one to go.
    let Some(_instance_lock) =
        instance::lock_with_retry(&instance::lock_path(), std::time::Duration::from_secs(3))
    else {
        eprintln!("clipboard-manager is already running");
        return;
    };

    if let Err(e) = App::new().and_then(|a| a.run()) {
        eprintln!("Error: {:#}", e);
        std::process::exit(1);
    }
}

/// GDK backend to force: XWayland on Wayland sessions where it is available,
/// unless the user chose a backend.
fn gdk_backend_for(wayland: bool, xwayland: bool, chosen: Option<&str>) -> Option<&'static str> {
    (wayland && xwayland && chosen.is_none()).then_some("x11")
}

/// Forward a command to the running instance over D-Bus and return the exit
/// status it reports. `show`/`toggle` start the daemon if none is running.
fn send_to_running(args: &[String], command: &cli::Command) -> i32 {
    use gtk4::gio;
    use gtk4::gio::prelude::*;

    let app = gio::Application::new(Some(&paths::application_id()), gio::ApplicationFlags::HANDLES_COMMAND_LINE);
    if let Err(e) = app.register(None::<&gio::Cancellable>) {
        eprintln!("clipboard-manager: cannot reach the session bus: {e}");
        return 1;
    }
    if app.is_remote() {
        let token = std::env::var("XDG_ACTIVATION_TOKEN")
            .or_else(|_| std::env::var("DESKTOP_STARTUP_ID"))
            .ok();
        let status = app.run_with_args(&cli::with_activation_token(args, token.as_deref())).value();
        if status != 0 {
            eprintln!("clipboard-manager: the running instance could not run '{}'", args[1..].join(" "));
        }
        return status;
    }
    // Nobody else owns the name: no instance is running.
    drop(app);
    match command {
        cli::Command::Show | cli::Command::Toggle => {
            let Ok(exe) = std::env::current_exe() else { return 1 };
            match spawn_daemon(&exe, true) {
                Ok(_) => 0,
                Err(e) => {
                    eprintln!("clipboard-manager: cannot start: {e}");
                    1
                }
            }
        }
        _ => {
            eprintln!("clipboard-manager is not running (start it with: clipboard-manager)");
            1
        }
    }
}

/// Stop the running instance (if any) and start a fresh one.
fn reload_daemon() {
    let was_running = is_running();
    if was_running {
        let args = vec!["clipboard-manager".to_string(), "quit".to_string()];
        send_to_running(&args, &cli::Command::Quit);
        // Wait for the old instance to release its D-Bus name and lock.
        for _ in 0..50 {
            if !is_running() && !instance::is_locked(&instance::lock_path()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    let Ok(exe) = std::env::current_exe() else {
        eprintln!("clipboard-manager: reload failed (cannot locate executable)");
        std::process::exit(1);
    };
    match spawn_daemon(&exe, false) {
        Ok(_) => println!("clipboard-manager: {}", if was_running { "reloaded" } else { "started" }),
        Err(e) => {
            eprintln!("clipboard-manager: reload failed: {e}");
            std::process::exit(1);
        }
    }
}

/// Whether an instance currently owns the application's D-Bus name.
fn is_running() -> bool {
    use gtk4::gio;
    let Ok(bus) = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>) else {
        return false;
    };
    bus.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
        Some(&(paths::application_id(),).to_variant()),
        Some(glib::VariantTy::new("(b)").unwrap()),
        gio::DBusCallFlags::NONE,
        1000,
        None::<&gio::Cancellable>,
    )
    .ok()
    .and_then(|v| v.get::<(bool,)>())
    .is_some_and(|(owned,)| owned)
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

    if spawn_daemon(&exe, false).is_ok() {
        std::process::exit(0);
    }
    // Spawn failed → fall through and run in foreground as a graceful fallback.
}

/// Start the detached daemon child. Its stderr (where the log goes) is
/// written to `$XDG_STATE_HOME/clipboard-manager/clipboard-manager.log`;
/// the previous run's log is kept as `clipboard-manager.log.1`.
/// `show_popup` opens the popup once the new instance is up.
fn spawn_daemon(exe: &std::path::Path, show_popup: bool) -> std::io::Result<std::process::Child> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::process::Stdio;
    let log = paths::state_dir().join("clipboard-manager.log");
    instance::rotate(&log);
    let stderr = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&log)
        .map(Stdio::from)
        .unwrap_or_else(|_| Stdio::null());
    let mut cmd = std::process::Command::new(exe);
    if show_popup {
        cmd.env("_CM_SHOW_ON_START", "1");
    }
    cmd.env("_CM_DAEMON", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .spawn()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wayland_sessions_use_xwayland_when_available() {
        assert_eq!(gdk_backend_for(true, true, None), Some("x11"));
        assert_eq!(gdk_backend_for(true, false, None), None); // no XWayland: native
        assert_eq!(gdk_backend_for(true, true, Some("wayland")), None); // user's choice wins
        assert_eq!(gdk_backend_for(false, true, None), None); // X11 session
    }
}
