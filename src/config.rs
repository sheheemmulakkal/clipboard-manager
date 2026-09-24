use anyhow::Result;
use serde::Deserialize;

// ── AppConfig defaults ────────────────────────────────────────────────────────

fn default_max_history() -> usize { 50 }
fn default_hotkey() -> String { "ctrl+alt+c".to_string() }
fn default_popup_width() -> i32 { 440 }
fn default_popup_height() -> i32 { 560 }
fn default_show_timestamps() -> bool { true }
fn default_deduplicate() -> bool { true }
fn default_popup_follow_cursor() -> bool { true }
fn default_clear_undo_timeout_secs() -> u64 { 5 }
fn default_max_text_bytes() -> usize { 1024 * 1024 }
fn default_tray_icon() -> bool { true }
fn default_expire_after_days() -> u64 { 0 }
fn default_ignore_apps() -> Vec<String> {
    [
        "keepassxc", "org.keepassxc.KeePassXC", "1password", "bitwarden", "Enpass",
        "seahorse", "gnome-keyring", "kwalletmanager5", "kwalletmanager",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

// ── SizeConfig defaults ───────────────────────────────────────────────────────

fn default_font_preview() -> u32 { 13 }
fn default_font_time() -> u32 { 11 }
fn default_font_title() -> u32 { 13 }
fn default_font_buttons() -> u32 { 13 }
fn default_font_undo() -> u32 { 12 }
fn default_row_height() -> u32 { 56 }

// ── Theme ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ThemeName {
    /// Built-in dark theme (default).
    #[default]
    Dark,
    /// Built-in light theme.
    Light,
    /// Follow the active GTK theme.
    System,
}

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
    /// Minimum row height in px. Default: 56.
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

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    #[serde(default = "default_max_history")]
    pub max_history: usize,
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    /// `"dark"` (default), `"light"` or `"system"` (follow the GTK theme).
    #[serde(default)]
    pub theme: ThemeName,
    #[serde(default = "default_popup_width")]
    pub popup_width: i32,
    #[serde(default = "default_popup_height")]
    pub popup_height: i32,
    #[serde(default = "default_show_timestamps")]
    pub show_timestamps: bool,
    #[serde(default = "default_deduplicate")]
    pub deduplicate: bool,
    #[serde(default = "default_popup_follow_cursor")]
    pub popup_follow_cursor: bool,
    #[serde(default = "default_clear_undo_timeout_secs")]
    pub clear_undo_timeout_secs: u64,
    /// Apps whose copies are never recorded (X11 WM_CLASS, case-insensitive).
    /// Default: common password managers.
    #[serde(default = "default_ignore_apps")]
    pub ignore_apps: Vec<String>,
    /// Delete unpinned items older than this many days. 0 = keep forever.
    #[serde(default = "default_expire_after_days")]
    pub expire_after_days: u64,
    /// Show an icon in the system tray. Default: true.
    #[serde(default = "default_tray_icon")]
    pub tray_icon: bool,
    /// Texts larger than this many bytes are not recorded. Default: 1 MiB.
    #[serde(default = "default_max_text_bytes")]
    pub max_text_bytes: usize,
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
            theme:                   ThemeName::default(),
            popup_width:             default_popup_width(),
            popup_height:            default_popup_height(),
            show_timestamps:         default_show_timestamps(),
            deduplicate:             default_deduplicate(),
            popup_follow_cursor:     default_popup_follow_cursor(),
            clear_undo_timeout_secs: default_clear_undo_timeout_secs(),
            max_text_bytes:          default_max_text_bytes(),
            tray_icon:               default_tray_icon(),
            expire_after_days:       default_expire_after_days(),
            ignore_apps:             default_ignore_apps(),
            colors:                  ColorConfig::default(),
            sizes:                   SizeConfig::default(),
        }
    }
}

impl AppConfig {
    /// Write the commented default config to `path`. Errors (e.g. a
    /// read-only filesystem) are logged, not fatal.
    pub fn write_default(path: &std::path::Path) {
        let result = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|_| std::fs::write(path, include_str!("../config/default.toml")));
        if let Err(e) = result {
            tracing::warn!("[config] cannot write {}: {e}", path.display());
        }
    }

    pub fn from_toml(text: &str) -> Result<Self> {
        Ok(toml::from_str(text)?)
    }

    /// Load the user's config. Never fails: on a read or parse error the
    /// defaults are used and the error text is returned for display.
    pub fn load() -> (Self, Option<String>) {
        let path = crate::paths::config_file();
        if path.exists() {
            let parsed = std::fs::read_to_string(&path)
                .map_err(anyhow::Error::from)
                .and_then(|text| Self::from_toml(&text));
            match parsed {
                Ok(config) => (config, None),
                Err(e) => (
                    AppConfig::default(),
                    Some(format!("{}: {e:#}", path.display())),
                ),
            }
        } else {
            // Write a default config on first run so the user has a file to edit.
            Self::write_default(&path);
            (AppConfig::default(), None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_gives_defaults() {
        let c = AppConfig::from_toml("").unwrap();
        assert_eq!(c.max_history, 50);
        assert_eq!(c.hotkey, "ctrl+alt+c");
    }

    #[test]
    fn shipped_default_config_matches_built_in_defaults() {
        let file = AppConfig::from_toml(include_str!("../config/default.toml")).unwrap();
        let d = AppConfig::default();
        assert_eq!(file.max_history, d.max_history);
        assert_eq!(file.hotkey, d.hotkey);
        assert_eq!(file.theme, d.theme);
        assert_eq!((file.popup_width, file.popup_height), (d.popup_width, d.popup_height));
        assert_eq!(file.max_text_bytes, d.max_text_bytes);
        assert_eq!(file.expire_after_days, d.expire_after_days);
        assert_eq!(file.ignore_apps, d.ignore_apps);
        assert_eq!(file.tray_icon, d.tray_icon);
        assert_eq!(file.clear_undo_timeout_secs, d.clear_undo_timeout_secs);
    }

    #[test]
    fn wrong_type_is_an_error() {
        assert!(AppConfig::from_toml("max_history = \"oops\"").is_err());
    }
}
