mod app;
mod clipboard;
mod config;
mod hotkey;
mod platform;
mod store;
mod ui;

use app::App;

fn main() {
    // Clipboard Manager requires an X11 session.
    // On Wayland, hotkeys and paste are not supported.
    // Allow GDK_BACKEND=x11 to override (runs via XWayland — hotkey and
    // paste work; cursor-follow positioning does not).
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

    if let Err(e) = App::new().and_then(|a| a.run()) {
        eprintln!("Error: {:#}", e);
        std::process::exit(1);
    }
}
