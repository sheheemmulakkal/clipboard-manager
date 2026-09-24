use crate::config::SizeConfig;
use crate::ui::theme::{Theme, COLORS};

/// Generate the full GTK4 CSS for the clipboard popup and its popovers.
pub fn generate_css(t: &Theme, sizes: &SizeConfig) -> String {
    let Theme {
        bg, surface, surface_hover, border, text, text_muted, accent, danger, hover,
        selection, shadow, ..
    } = t;

    let fp     = sizes.font_preview;
    let ft     = sizes.font_time;
    let ftitle = sizes.font_title;
    let fb     = sizes.font_buttons;
    let fu     = sizes.font_undo;
    let rh     = sizes.row_height;

    let mut css = format!(
        r#"
/* ── Window & card ───────────────────────────────────────────────────── */
window.cm-popup {{
    background: transparent;
}}
.popup-card {{
    background-color: {bg};
    color: {text};
    border: 1px solid {border};
    border-radius: 14px;
    margin: 12px;
    box-shadow: 0 12px 32px {shadow}, 0 0 0 1px rgba(0, 0, 0, 0.12);
}}
window.no-compositing .popup-card {{
    margin: 0;
    border-radius: 0;
    box-shadow: none;
}}

/* ── Header ──────────────────────────────────────────────────────────── */
.popup-header {{
    padding: 12px 12px 8px 12px;
}}
.app-icon {{
    background-color: {surface};
    border-radius: 10px;
    min-width: 36px;
    min-height: 36px;
    margin-right: 6px;
}}
.popup-title {{
    color: {text};
    font-size: {title_px}px;
    font-weight: 700;
}}
.paused-badge {{
    color: {accent};
    background-color: alpha({accent}, 0.15);
    border-radius: 999px;
    padding: 2px 8px;
    margin: 0 6px;
    font-size: 11px;
    font-weight: 600;
}}
.header-btn, .close-btn {{
    min-width: 34px;
    min-height: 34px;
    padding: 0;
    border-radius: 10px;
    background: transparent;
    border: none;
    box-shadow: none;
}}
.header-btn:hover, .header-btn:checked {{
    background-color: {surface};
}}
.close-btn {{
    border-radius: 17px;
    background-color: {surface};
    margin-left: 4px;
}}
.close-btn:hover {{
    background-color: {surface_hover};
}}

/* ── Search ──────────────────────────────────────────────────────────── */
.search-box {{
    background-color: {surface};
    border: 1px solid {border};
    border-radius: 10px;
    margin: 2px 12px 10px 12px;
    padding: 0 6px 0 4px;
    min-height: 40px;
    transition: border-color 150ms ease;
}}
.search-box:focus-within {{
    border-color: alpha({accent}, 0.7);
}}
.search-box entry, .search-box searchentry, .search-box text {{
    background: transparent;
    border: none;
    box-shadow: none;
    outline: none;
    color: {text};
    font-size: {search_px}px;
    min-height: 38px;
}}
.search-box image {{
    color: {text_muted};
}}
.kbd-chip {{
    color: {text_muted};
    background-color: {surface_hover};
    border: 1px solid {border};
    border-radius: 6px;
    padding: 2px 8px;
    font-size: 12px;
}}

/* ── List ────────────────────────────────────────────────────────────── */
scrolledwindow, viewport {{
    background: transparent;
}}
list.history {{
    background: transparent;
    padding: 0 8px 64px 8px;
}}
list.history > row {{
    background: transparent;
    border-bottom: 1px solid {border};
    padding: 0;
    outline: none;
    transition: background-color 120ms ease;
}}
list.history > row:hover {{
    background-color: {hover};
}}
list.history > row:selected, list.history > row:selected:hover {{
    background-color: {selection};
    border-radius: 12px;
}}
list.history > row:focus-visible {{
    box-shadow: inset 0 0 0 1px alpha({accent}, 0.5);
    border-radius: 12px;
}}
list.history > row.pinned {{
    border: 1.5px solid {accent};
    border-radius: 12px;
    background-color: alpha({accent}, 0.06);
    margin: 2px 0;
}}
list.history > row.pinned:selected {{
    background-color: alpha({accent}, 0.14);
}}

.item-row {{
    padding: 8px 8px 8px 6px;
    min-height: {rh}px;
}}
.color-dot {{
    min-width: 8px;
    min-height: 8px;
    border-radius: 4px;
    margin: 0 6px 0 4px;
}}
.kind-tile {{
    background-color: {surface};
    border-radius: 10px;
    min-width: 40px;
    min-height: 40px;
}}
.thumb-frame {{
    border-radius: 8px;
    background-color: {surface};
}}
.row-title {{
    color: {text};
    font-size: {title_row_px}px;
    font-weight: 600;
}}
.row-subtitle {{
    color: {text_muted};
    font-size: {fp}px;
}}
.tag-pill {{
    border-radius: 999px;
    padding: 3px 10px;
    font-size: 11.5px;
    font-weight: 600;
}}
.time-label {{
    color: {text_muted};
    font-size: {ft}px;
    margin: 0 4px;
}}
.row-btn, .pin-toggle {{
    min-width: 30px;
    min-height: 30px;
    padding: 0;
    border-radius: 8px;
    background: transparent;
    border: none;
    box-shadow: none;
    font-size: {fb}px;
}}
.row-btn:hover, .pin-toggle:hover {{
    background-color: {surface_hover};
}}
.row-btn.danger:hover {{
    background-color: alpha({danger}, 0.16);
}}
.quick-index {{
    color: {bg};
    background-color: {accent};
    border-radius: 6px;
    min-width: 16px;
    font-size: 10px;
    font-weight: 700;
    padding: 0 3px;
    margin: 0 2px 0 0;
}}
.empty-label {{
    color: {text_muted};
    font-size: {fp}px;
}}

/* ── Undo bar ────────────────────────────────────────────────────────── */
.undo-bar {{
    background-color: {surface};
    border-top: 1px solid {border};
    padding: 8px 12px;
}}
.undo-label {{
    color: {text};
    font-size: {fu}px;
}}
.undo-btn {{
    color: {accent};
    background: transparent;
    border: 1px solid alpha({accent}, 0.5);
    border-radius: 8px;
    padding: 3px 12px;
    font-size: {fu}px;
    box-shadow: none;
}}
.undo-btn:hover {{
    background-color: alpha({accent}, 0.12);
}}

/* ── Scroll-to-top button ─────────────────────────────────────────────── */
.scroll-top-btn {{
    min-width: 38px;
    min-height: 38px;
    padding: 0;
    border-radius: 19px;
    background-color: {accent};
    border: none;
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.35);
}}
.scroll-top-btn:hover {{
    background-color: shade({accent}, 1.1);
}}

/* ── Menus & popovers ────────────────────────────────────────────────── */
popover.cm-menu > contents, popover.cm-editor > contents {{
    background-color: {bg};
    color: {text};
    border: 1px solid {border};
    border-radius: 12px;
    padding: 6px;
    box-shadow: 0 10px 28px {shadow};
}}
popover.cm-editor > contents {{
    padding: 12px;
}}
.menu-item {{
    background: transparent;
    border: none;
    box-shadow: none;
    border-radius: 8px;
    padding: 6px 10px;
    min-height: 30px;
    min-width: 200px;
}}
.menu-item:hover, .menu-item:focus-visible {{
    background-color: {surface_hover};
}}
.menu-label {{
    color: {text};
    font-size: 13.5px;
}}
.menu-accel {{
    color: {text_muted};
    font-size: 12px;
    margin-left: 16px;
}}
.menu-item.danger .menu-label, .menu-item.danger .menu-accel {{
    color: {danger};
}}
.menu-header {{
    color: {text_muted};
    font-size: 12px;
    padding: 4px 10px 6px 10px;
}}
separator.menu-sep {{
    background-color: {border};
    min-height: 1px;
    margin: 4px 6px;
}}
.popover-form-label {{
    color: {text_muted};
    font-size: 12px;
}}
popover.cm-editor entry, popover.cm-editor textview, popover.cm-editor text {{
    background-color: {surface};
    color: {text};
    border-radius: 8px;
}}
popover.cm-editor entry {{
    border: 1px solid {border};
    box-shadow: none;
    min-height: 32px;
}}
.editor-scroll {{
    border: 1px solid {border};
    border-radius: 8px;
    background-color: {surface};
}}
.preview-text {{
    font-family: monospace;
    font-size: 12.5px;
    color: {text};
}}
.primary-btn {{
    background-color: {accent};
    color: white;
    border: none;
    border-radius: 8px;
    padding: 4px 14px;
    box-shadow: none;
}}
.primary-btn:hover {{
    background-color: shade({accent}, 1.1);
}}
.secondary-btn {{
    background-color: {surface};
    color: {text};
    border: 1px solid {border};
    border-radius: 8px;
    padding: 4px 14px;
    box-shadow: none;
}}
"#,
        title_px     = ftitle + 3,
        search_px    = fp + 1,
        title_row_px = fp + 1,
    );

    // Entry colours: dots, menu swatches and tag pills.
    for (name, hex) in COLORS {
        css.push_str(&format!(
            ".dot-{name} {{ background-color: {hex}; }}\n\
             .tag-{name} {{ background-color: alpha({hex}, 0.18); color: mix({hex}, white, 0.25); }}\n"
        ));
    }
    css.push_str(&format!(".dot-none {{ background-color: alpha({text_muted}, 0.6); }}\n"));
    css
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ColorConfig, ThemeName};

    #[test]
    fn css_contains_theme_tokens_and_palette_classes() {
        let t = Theme::resolve(ThemeName::Dark, &ColorConfig::default());
        let css = generate_css(&t, &SizeConfig::default());
        assert!(css.contains("#f97316"));
        assert!(css.contains(".dot-purple"));
        assert!(css.contains(".tag-green"));
        assert!(!css.contains("{{"));
    }
}
