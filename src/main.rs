mod app;
mod clipboard;
mod config;
mod hotkey;
mod paste;
mod store;
mod ui;

use config::AppConfig;

fn main() -> anyhow::Result<()> {
    let config = AppConfig::load()?;
    println!("Config loaded. Max history: {}", config.max_history);
    Ok(())
}
