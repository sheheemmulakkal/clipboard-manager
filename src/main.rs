mod app;
mod clipboard;
mod config;
mod hotkey;
mod paste;
mod store;
mod tray;
mod ui;

use app::App;

fn main() {
    if let Err(e) = App::new().and_then(|a| a.run()) {
        eprintln!("Error: {:#}", e);
        std::process::exit(1);
    }
}
