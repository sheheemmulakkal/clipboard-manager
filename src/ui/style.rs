use crate::config::{ColorConfig, SizeConfig};

/// Generate the full GTK4 CSS for the clipboard popup.
///
/// Each color slot falls back to the active system theme variable when the
/// user has not provided an override. Smart derivation is applied so that
/// setting a base color (e.g. `text`) automatically informs related derived
/// slots (e.g. `text_muted`, `row_hover`) unless those are also overridden.
pub fn generate_css(colors: &ColorConfig, sizes: &SizeConfig) -> String {
    // ── Resolve each color slot ──────────────────────────────────────────────

    let bg = colors.background.as_deref()
        .unwrap_or("@theme_bg_color")
        .to_string();

    let header_bg = colors.header_background.as_deref()
        .map(str::to_string)
        .unwrap_or_else(|| format!("shade({bg}, 0.92)"));

    let border = colors.border.as_deref()
        .unwrap_or("@borders")
        .to_string();

    let text = colors.text.as_deref()
        .unwrap_or("@theme_fg_color")
        .to_string();

    // Derived from `text` if not explicitly set.
    let text_muted = colors.text_muted.as_deref()
        .map(str::to_string)
        .unwrap_or_else(|| format!("alpha({text}, 0.5)"));

    let accent = colors.accent.as_deref()
        .unwrap_or("@theme_selected_bg_color")
        .to_string();

    let error = colors.error.as_deref()
        .unwrap_or("@error_color")
        .to_string();

    // Derived from `text` if not explicitly set.
    let row_hover = colors.row_hover.as_deref()
        .map(str::to_string)
        .unwrap_or_else(|| format!("alpha({text}, 0.06)"));

    // Derived from `accent` if not explicitly set.
    let selection = colors.selection.as_deref()
        .map(str::to_string)
        .unwrap_or_else(|| format!("alpha({accent}, 0.25)"));

    // Further derived — not individually configurable, derived from base slots.
    let btn_hover_bg      = format!("alpha({text}, 0.12)");
    let pin_active_hover  = format!("alpha({accent}, 0.15)");
    let error_hover_bg    = format!("alpha({error}, 0.12)");
    let error_border      = error.clone();

    // ── Sizes ────────────────────────────────────────────────────────────────
    let fp     = sizes.font_preview;
    let ft     = sizes.font_time;
    let ftitle = sizes.font_title;
    let fb     = sizes.font_buttons;
    let fu     = sizes.font_undo;
    let rh     = sizes.row_height;

    // ── Emit CSS ─────────────────────────────────────────────────────────────
    format!(
        r#"
window {{
    background-color: {bg};
    border-radius: 10px;
    border: 1px solid {border};
}}

/* ── Custom drag handle ──────────────────────────────────────────────── */
.popup-header {{
    background-color: {header_bg};
    padding: 10px 14px;
    border-radius: 9px 9px 0 0;
    border-bottom: 1px solid {border};
}}

.popup-title {{
    color: {text};
    font-size: {ftitle}px;
    font-weight: bold;
}}

/* ── Clear All button (in header) ───────────────────────────────────── */
.clear-btn {{
    font-size: 11px;
    padding: 3px 9px;
    border-radius: 5px;
    background-color: transparent;
    border: 1px solid {border};
    color: {text_muted};
    box-shadow: none;
    transition: color 150ms ease, border-color 150ms ease, background-color 150ms ease;
}}

.clear-btn:hover {{
    color: {error};
    border-color: {error_border};
    background-color: {error_hover_bg};
}}

/* ── List ────────────────────────────────────────────────────────────── */
list {{
    background-color: {bg};
}}

list > row {{
    background-color: {bg};
    outline: none;
}}

.item-row {{
    padding: 8px 10px;
    border-bottom: 1px solid {border};
    min-height: {rh}px;
}}

/* Pinned rows get a subtle left accent */
.item-row-pinned {{
    border-left: 3px solid {accent};
    padding-left: 7px;
}}

.item-row:hover {{
    background-color: {row_hover};
}}

list > row:selected,
list > row:selected:hover {{
    background-color: {selection};
    outline: none;
}}

list > row:focus {{
    outline: none;
}}

/* ── Text labels ─────────────────────────────────────────────────────── */
.preview-label {{
    color: {text};
    font-size: {fp}px;
}}

.time-label {{
    color: {text_muted};
    font-size: {ft}px;
    margin-left: 4px;
}}

/* ── Pin indicator (small icon shown on pinned rows) ─────────────────── */
.pin-indicator {{
    color: {accent};
    font-size: {ft}px;
    margin-right: 4px;
}}

/* ── Undo bar (appears at the bottom after Clear All) ───────────────── */
.undo-bar {{
    padding: 8px 12px;
    background-color: {header_bg};
    border-top: 1px solid {border};
}}

.undo-label {{
    color: {text};
    font-size: {fu}px;
}}

.undo-btn {{
    font-size: {fu}px;
    padding: 3px 10px;
    border-radius: 5px;
    background-color: {btn_hover_bg};
    border: 1px solid {border};
    color: {text};
    box-shadow: none;
    transition: background-color 150ms ease;
}}

.undo-btn:hover {{
    background-color: alpha({text}, 0.2);
}}

/* ── Row action buttons ──────────────────────────────────────────────── */
.row-actions {{
    opacity: 0;
    transition: opacity 200ms ease;
}}

/* Show buttons when the row is hovered or selected */
.item-row:hover .row-actions,
row:selected .row-actions {{
    opacity: 1;
}}

.row-btn {{
    min-width: 0;
    min-height: 0;
    padding: 2px 5px;
    border-radius: 5px;
    font-size: {fb}px;
    background: transparent;
    border: none;
    box-shadow: none;
    color: {text_muted};
    transition: color 150ms ease, background-color 150ms ease;
}}

.row-btn:hover {{
    background-color: {btn_hover_bg};
    color: {text};
}}

/* Pin button — highlighted when item is pinned */
.pin-btn-active {{
    color: {accent};
}}

.pin-btn-active:hover {{
    color: {accent};
    background-color: {pin_active_hover};
}}

/* Delete button — destructive color on hover */
.del-btn:hover {{
    color: {error};
    background-color: {error_hover_bg};
}}

/* ── Search bar ──────────────────────────────────────────────────────── */
.search-bar {{
    border-bottom: 1px solid {border};
    background-color: {bg};
}}

.search-entry {{
    background-color: {row_hover};
    color: {text};
    border: 1px solid {border};
    border-radius: 6px;
    box-shadow: none;
    font-size: {fp}px;
    min-height: 32px;
}}

.search-entry:focus {{
    border-color: {accent};
    box-shadow: 0 0 0 1px {accent};
    outline: none;
}}

/* Hint label: "Esc to clear" — appears when search entry has text */
.search-hint {{
    color: {text_muted};
    font-size: 10px;
    border: 1px solid {border};
    border-radius: 4px;
    padding: 1px 5px;
}}

/* ── Empty search results ────────────────────────────────────────────── */
.empty-label {{
    color: {text_muted};
    font-size: {fp}px;
    font-style: italic;
}}

/* ── User label tag (shown below the preview when a label is set) ───── */
.label-tag {{
    color: {text_muted};
    font-size: smaller;
}}

/* ── Color accent borders — override pinned border when both present ── */
.item-row-color-red    {{ border-left: 3px solid #f38ba8; padding-left: 7px; }}
.item-row-color-pink   {{ border-left: 3px solid #f2cdcd; padding-left: 7px; }}
.item-row-color-mauve  {{ border-left: 3px solid #cba6f7; padding-left: 7px; }}
.item-row-color-blue   {{ border-left: 3px solid #89b4fa; padding-left: 7px; }}
.item-row-color-teal   {{ border-left: 3px solid #94e2d5; padding-left: 7px; }}
.item-row-color-green  {{ border-left: 3px solid #a6e3a1; padding-left: 7px; }}
.item-row-color-yellow {{ border-left: 3px solid #f9e2af; padding-left: 7px; }}
.item-row-color-peach  {{ border-left: 3px solid #fab387; padding-left: 7px; }}

/* ── Right-click label popover ───────────────────────────────────────── */
.popover-form-label {{
    color: {text_muted};
    font-size: smaller;
    min-width: 40px;
}}

.apply-btn {{
    font-size: 11px;
    padding: 3px 12px;
    border-radius: 5px;
    background-color: {btn_hover_bg};
    border: 1px solid {border};
    color: {text};
    box-shadow: none;
    transition: background-color 150ms ease;
}}

.apply-btn:hover {{
    background-color: alpha({text}, 0.2);
}}

/* Swatch base */
.color-swatch {{
    min-width: 18px;
    min-height: 18px;
    border-radius: 50%;
    padding: 0;
    border: 2px solid transparent;
    box-shadow: none;
}}

.color-swatch:hover      {{ border-color: alpha({text}, 0.4); }}
.color-swatch-active     {{ border-color: {text}; }}

/* "none" button is wider and pill-shaped */
.color-swatch-none {{
    min-width: 32px;
    border-radius: 4px;
    font-size: 10px;
    background: alpha({text}, 0.08);
    color: {text_muted};
}}

/* Per-color swatch backgrounds */
.color-swatch-red    {{ background: #f38ba8; }}
.color-swatch-pink   {{ background: #f2cdcd; }}
.color-swatch-mauve  {{ background: #cba6f7; }}
.color-swatch-blue   {{ background: #89b4fa; }}
.color-swatch-teal   {{ background: #94e2d5; }}
.color-swatch-green  {{ background: #a6e3a1; }}
.color-swatch-yellow {{ background: #f9e2af; }}
.color-swatch-peach  {{ background: #fab387; }}
"#
    )
}
