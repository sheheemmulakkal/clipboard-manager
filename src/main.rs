mod app;
mod clipboard;
mod config;
mod hotkey;
mod paste;
mod store;
mod ui;

use app::App;

fn main() -> anyhow::Result<()> {
    App::new()?.run()
}
