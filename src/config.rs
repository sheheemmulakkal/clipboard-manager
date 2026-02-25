use anyhow::Result;
use serde::Deserialize;
use std::path::PathBuf;

fn default_max_history() -> usize {
    50
}
fn default_hotkey() -> String {
    "ctrl+alt+c".to_string()
}
fn default_popup_width() -> i32 {
    420
}
fn default_popup_max_items() -> usize {
    20
}
fn default_show_timestamps() -> bool {
    true
}
fn default_deduplicate() -> bool {
    true
}
fn default_popup_follow_cursor() -> bool {
    true
}
fn default_clear_undo_timeout_secs() -> u64 {
    5
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct AppConfig {
    #[serde(default = "default_max_history")]
    pub max_history: usize,
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    #[serde(default = "default_popup_width")]
    pub popup_width: i32,
    #[serde(default = "default_popup_max_items")]
    pub popup_max_items: usize,
    #[serde(default = "default_show_timestamps")]
    pub show_timestamps: bool,
    #[serde(default = "default_deduplicate")]
    pub deduplicate: bool,
    #[serde(default = "default_popup_follow_cursor")]
    pub popup_follow_cursor: bool,
    #[serde(default = "default_clear_undo_timeout_secs")]
    pub clear_undo_timeout_secs: u64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            max_history: default_max_history(),
            hotkey: default_hotkey(),
            popup_width: default_popup_width(),
            popup_max_items: default_popup_max_items(),
            show_timestamps: default_show_timestamps(),
            deduplicate: default_deduplicate(),
            popup_follow_cursor:     default_popup_follow_cursor(),
            clear_undo_timeout_secs: default_clear_undo_timeout_secs(),
        }
    }
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let path = Self::config_path();
        if path.exists() {
            let text = std::fs::read_to_string(&path)?;
            let config: AppConfig = toml::from_str(&text)?;
            Ok(config)
        } else {
            Ok(AppConfig::default())
        }
    }

    fn config_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home)
            .join(".config")
            .join("clipboard-manager")
            .join("config.toml")
    }
}
