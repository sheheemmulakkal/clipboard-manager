//! Colour tokens for the popup, the entry colour palette and default tags.

use crate::config::{ColorConfig, ThemeName};

/// Entry colours offered in the UI (name, hex). Order = menu order.
pub const COLORS: [(&str, &str); 8] = [
    ("red",    "#ef4444"),
    ("orange", "#f97316"),
    ("yellow", "#eab308"),
    ("green",  "#22c55e"),
    ("blue",   "#3b82f6"),
    ("purple", "#a855f7"),
    ("pink",   "#ec4899"),
    ("gray",   "#9ca3af"),
];

/// Tags offered before the user has created any (name, colour name).
pub const DEFAULT_TAGS: [(&str, &str); 5] = [
    ("Work",     "green"),
    ("Personal", "blue"),
    ("Security", "purple"),
    ("Ideas",    "pink"),
    ("Snippets", "orange"),
];

/// Map a stored colour name (including names from the v1 palette) to the
/// current palette.
pub fn normalize_color(name: &str) -> Option<&'static str> {
    let name = match name {
        "mauve" => "purple",
        "peach" => "orange",
        "teal"  => "blue",
        other   => other,
    };
    COLORS.iter().find(|(n, _)| *n == name).map(|(n, _)| *n)
}

pub fn color_hex(name: &str) -> Option<&'static str> {
    let name = normalize_color(name)?;
    COLORS.iter().find(|(n, _)| *n == name).map(|(_, h)| *h)
}

/// Colour name for a tag pill: the default mapping for built-in tags,
/// otherwise a stable pick from the palette based on the tag text.
pub fn tag_color(tag: &str) -> &'static str {
    if let Some((_, c)) = DEFAULT_TAGS.iter().find(|(t, _)| t.eq_ignore_ascii_case(tag)) {
        return c;
    }
    // FNV-1a — stable across runs and platforms (unlike DefaultHasher).
    let mut h: u32 = 0x811c_9dc5;
    for b in tag.to_lowercase().bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    COLORS[(h as usize) % (COLORS.len() - 1)].0 // never gray
}

/// Resolved colour tokens (CSS colour expressions).
#[derive(Clone, Debug)]
pub struct Theme {
    pub bg:            String,
    pub surface:       String,
    pub surface_hover: String,
    pub border:        String,
    pub text:          String,
    pub text_muted:    String,
    pub accent:        String,
    pub danger:        String,
    pub hover:         String,
    pub selection:     String,
    pub shadow:        String,
    /// Concrete colour for icons (SVGs can't use GTK named colours).
    pub icon:          String,
    pub icon_muted:    String,
}

impl Theme {
    pub fn resolve(name: ThemeName, o: &ColorConfig) -> Theme {
        let base = match name {
            ThemeName::Dark => Theme {
                bg:            "#1b1d22".into(),
                surface:       "#25282f".into(),
                surface_hover: "#2d313a".into(),
                border:        "rgba(255,255,255,0.08)".into(),
                text:          "#eceef1".into(),
                text_muted:    "#9aa0a8".into(),
                accent:        "#f97316".into(),
                danger:        "#ef4444".into(),
                hover:         "rgba(255,255,255,0.04)".into(),
                selection:     "rgba(249,115,22,0.12)".into(),
                shadow:        "rgba(0,0,0,0.55)".into(),
                icon:          "#d6d9de".into(),
                icon_muted:    "#8b919a".into(),
            },
            ThemeName::Light => Theme {
                bg:            "#ffffff".into(),
                surface:       "#f1f2f4".into(),
                surface_hover: "#e7e9ec".into(),
                border:        "rgba(0,0,0,0.09)".into(),
                text:          "#1f2328".into(),
                text_muted:    "#6b7280".into(),
                accent:        "#ea580c".into(),
                danger:        "#dc2626".into(),
                hover:         "rgba(0,0,0,0.035)".into(),
                selection:     "rgba(234,88,12,0.10)".into(),
                shadow:        "rgba(0,0,0,0.18)".into(),
                icon:          "#3b4048".into(),
                icon_muted:    "#7b818a".into(),
            },
            ThemeName::System => Theme {
                bg:            "@theme_bg_color".into(),
                surface:       "alpha(@theme_fg_color, 0.06)".into(),
                surface_hover: "alpha(@theme_fg_color, 0.10)".into(),
                border:        "@borders".into(),
                text:          "@theme_fg_color".into(),
                text_muted:    "alpha(@theme_fg_color, 0.55)".into(),
                accent:        "@theme_selected_bg_color".into(),
                danger:        "@error_color".into(),
                hover:         "alpha(@theme_fg_color, 0.04)".into(),
                selection:     "alpha(@theme_selected_bg_color, 0.18)".into(),
                shadow:        "rgba(0,0,0,0.35)".into(),
                // Replaced at runtime with the theme's real foreground colour.
                icon:          "#808080".into(),
                icon_muted:    "#808080".into(),
            },
        };
        let pick = |over: &Option<String>, dflt: String| over.clone().unwrap_or(dflt);
        let text = pick(&o.text, base.text);
        let accent = pick(&o.accent, base.accent);
        Theme {
            bg:            pick(&o.background, base.bg),
            surface:       pick(&o.header_background, base.surface),
            surface_hover: base.surface_hover,
            border:        pick(&o.border, base.border),
            text_muted:    o.text_muted.clone().unwrap_or_else(|| {
                if o.text.is_some() { format!("alpha({text}, 0.6)") } else { base.text_muted }
            }),
            selection:     o.selection.clone().unwrap_or_else(|| {
                if o.accent.is_some() { format!("alpha({accent}, 0.14)") } else { base.selection }
            }),
            hover:         pick(&o.row_hover, base.hover),
            danger:        pick(&o.error, base.danger),
            shadow:        base.shadow,
            icon:          o.text.clone().filter(|t| t.starts_with('#')).unwrap_or(base.icon),
            icon_muted:    o.text_muted.clone().filter(|t| t.starts_with('#')).unwrap_or(base.icon_muted),
            text,
            accent,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, ColorConfig, ThemeName};

    #[test]
    fn legacy_config_still_parses() {
        let c = AppConfig::from_toml(
            "popup_max_items = 20\nnerd_font = true\n[colors]\naccent = \"#123456\"\n",
        )
        .unwrap();
        assert_eq!(c.theme, ThemeName::Dark);
        assert_eq!(c.colors.accent.as_deref(), Some("#123456"));
    }

    #[test]
    fn theme_names_parse() {
        let c = AppConfig::from_toml("theme = \"system\"").unwrap();
        assert_eq!(c.theme, ThemeName::System);
        assert!(AppConfig::from_toml("theme = \"neon\"").is_err());
    }

    #[test]
    fn overrides_win_over_theme() {
        let colors = ColorConfig { accent: Some("#123456".into()), ..Default::default() };
        let t = Theme::resolve(ThemeName::Dark, &colors);
        assert_eq!(t.accent, "#123456");
        assert_eq!(t.bg, Theme::resolve(ThemeName::Dark, &ColorConfig::default()).bg);
    }

    #[test]
    fn legacy_color_names_map_to_palette() {
        assert_eq!(normalize_color("mauve"), Some("purple"));
        assert_eq!(normalize_color("peach"), Some("orange"));
        assert_eq!(normalize_color("teal"), Some("blue"));
        assert_eq!(normalize_color("green"), Some("green"));
        assert_eq!(normalize_color("nonsense"), None);
        assert_eq!(color_hex("purple"), Some("#a855f7"));
    }

    #[test]
    fn tag_colors_default_and_stable() {
        assert_eq!(tag_color("Work"), "green");
        assert_eq!(tag_color("work"), "green");
        assert_eq!(tag_color("Snippets"), "orange");
        let c = tag_color("Groceries");
        assert_eq!(c, tag_color("Groceries"));
        assert!(COLORS.iter().any(|(n, _)| *n == c));
    }
}
