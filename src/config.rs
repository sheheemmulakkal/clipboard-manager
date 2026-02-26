use anyhow::Result;
use serde::Deserialize;
use std::path::PathBuf;

// ── AppConfig defaults ────────────────────────────────────────────────────────

fn default_max_history() -> usize { 50 }
fn default_hotkey() -> String { "ctrl+alt+c".to_string() }
fn default_popup_width() -> i32 { 420 }
fn default_popup_max_items() -> usize { 20 }
fn default_show_timestamps() -> bool { true }
fn default_deduplicate() -> bool { true }
fn default_popup_follow_cursor() -> bool { true }
fn default_clear_undo_timeout_secs() -> u64 { 5 }
fn default_nerd_font() -> bool { false }

// ── SizeConfig defaults ───────────────────────────────────────────────────────

fn default_font_preview() -> u32 { 13 }
fn default_font_time() -> u32 { 11 }
fn default_font_title() -> u32 { 13 }
fn default_font_buttons() -> u32 { 13 }
fn default_font_undo() -> u32 { 12 }
fn default_row_height() -> u32 { 44 }

// ── Color overrides ───────────────────────────────────────────────────────────

/// All fields are optional. Unset fields fall back to the active GTK4 system
/// theme. Smart derivation: if `text` is set but `text_muted` / `row_hover`
/// are not, they are derived from `text`. Same for `accent` → `selection`.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct ColorConfig {
    /// Main window / list background. Default: system @theme_bg_color.
    pub background:        Option<String>,
    /// Header and undo-bar background. Default: shade(background, 0.92).
    pub header_background: Option<String>,
    /// All border lines. Default: system @borders.
    pub border:            Option<String>,
    /// Primary text (preview, title). Default: system @theme_fg_color.
    pub text:              Option<String>,
    /// Muted text (timestamps, inactive buttons). Default: alpha(text, 0.5).
    pub text_muted:        Option<String>,
    /// Accent color (pin highlight, selection tint). Default: system @theme_selected_bg_color.
    pub accent:            Option<String>,
    /// Destructive hover color (delete, clear). Default: system @error_color.
    pub error:             Option<String>,
    /// Row hover background. Default: alpha(text, 0.06).
    pub row_hover:         Option<String>,
    /// Selected row background. Default: alpha(accent, 0.25).
    pub selection:         Option<String>,
}

// ── Size overrides ────────────────────────────────────────────────────────────

/// All sizes are in CSS px units. Use the `[sizes]` section in config.toml.
#[derive(Debug, Deserialize, Clone)]
pub struct SizeConfig {
    /// Clipboard item preview text. Default: 13.
    #[serde(default = "default_font_preview")]
    pub font_preview: u32,
    /// Timestamp / age label. Default: 11.
    #[serde(default = "default_font_time")]
    pub font_time: u32,
    /// Popup header title. Default: 13.
    #[serde(default = "default_font_title")]
    pub font_title: u32,
    /// Action button icons. Default: 13.
    #[serde(default = "default_font_buttons")]
    pub font_buttons: u32,
    /// Undo bar text. Default: 12.
    #[serde(default = "default_font_undo")]
    pub font_undo: u32,
    /// Minimum row height in px. Default: 44.
    #[serde(default = "default_row_height")]
    pub row_height: u32,
}

impl Default for SizeConfig {
    fn default() -> Self {
        Self {
            font_preview: default_font_preview(),
            font_time:    default_font_time(),
            font_title:   default_font_title(),
            font_buttons: default_font_buttons(),
            font_undo:    default_font_undo(),
            row_height:   default_row_height(),
        }
    }
}

// ── AppConfig ─────────────────────────────────────────────────────────────────

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
    /// Use Nerd Font icons for action buttons. Requires a Nerd Font to be
    /// installed and set as the application font. Default: false.
    #[serde(default = "default_nerd_font")]
    pub nerd_font: bool,
    /// Optional color overrides. Unset fields use the active GTK4 system theme.
    #[serde(default)]
    pub colors: ColorConfig,
    /// Optional size overrides (px). Unset fields use built-in defaults.
    #[serde(default)]
    pub sizes: SizeConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            max_history:             default_max_history(),
            hotkey:                  default_hotkey(),
            popup_width:             default_popup_width(),
            popup_max_items:         default_popup_max_items(),
            show_timestamps:         default_show_timestamps(),
            deduplicate:             default_deduplicate(),
            popup_follow_cursor:     default_popup_follow_cursor(),
            clear_undo_timeout_secs: default_clear_undo_timeout_secs(),
            nerd_font:               default_nerd_font(),
            colors:                  ColorConfig::default(),
            sizes:                   SizeConfig::default(),
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
