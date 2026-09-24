mod app;
mod cli;
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
use glib::prelude::ToVariant;

fn main() {
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

/// Forward a command to the running instance over D-Bus and return the exit
/// status it reports. `show`/`toggle` start the daemon if none is running.
fn send_to_running(args: &[String], command: &cli::Command) -> i32 {
    use gtk4::gio;
    use gtk4::gio::prelude::*;

    let app = gio::Application::new(Some(app::APP_ID), gio::ApplicationFlags::HANDLES_COMMAND_LINE);
    if let Err(e) = app.register(None::<&gio::Cancellable>) {
        eprintln!("clipboard-manager: cannot reach the session bus: {e}");
        return 1;
    }
    if app.is_remote() {
        let status = app.run_with_args(args).value();
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
        // Wait for the old instance to release its D-Bus name.
        for _ in 0..50 {
            if !is_running() {
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
        Some(&(app::APP_ID,).to_variant()),
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
/// written to `$XDG_STATE_HOME/clipboard-manager/clipboard-manager.log`,
/// truncated on every start, so problems are diagnosable after the fact.
/// `show_popup` opens the popup once the new instance is up.
fn spawn_daemon(exe: &std::path::Path, show_popup: bool) -> std::io::Result<std::process::Child> {
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
