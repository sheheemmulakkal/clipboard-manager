//! Text shown in history rows (pure, no GTK).

use crate::clipboard::entry::{ClipboardContent, ClipboardEntry};
use crate::clipboard::kind::ContentKind;

/// Longest subtitle we hand to GTK (it ellipsizes to the row width anyway).
const SUBTITLE_MAX_CHARS: usize = 200;

/// "just now", "5 min ago", "2 hours ago", "yesterday", "3 months ago", …
pub fn relative_time(copied_at: u64, now: u64) -> String {
    const MIN: u64 = 60;
    const HOUR: u64 = 60 * MIN;
    const DAY: u64 = 24 * HOUR;
    let secs = now.saturating_sub(copied_at);
    let plural = |n: u64, unit: &str| {
        if n == 1 { format!("1 {unit} ago") } else { format!("{n} {unit}s ago") }
    };
    match secs {
        s if s < 10 => "just now".into(),
        s if s < MIN => format!("{s} sec ago"),
        s if s < HOUR => format!("{} min ago", s / MIN),
        s if s < DAY => plural(s / HOUR, "hour"),
        s if s < 2 * DAY => "yesterday".into(),
        s if s < 30 * DAY => format!("{} days ago", s / DAY),
        s if s < 365 * DAY => plural(s / (30 * DAY), "month"),
        s => plural(s / (365 * DAY), "year"),
    }
}

fn has_label(e: &ClipboardEntry) -> bool {
    e.label.as_deref().is_some_and(|l| !l.trim().is_empty())
}

/// Bold first line of a row: the user's label, else the content itself
/// (images: "Image" / "Screenshot"). The kind is shown by the icon.
pub fn title(e: &ClipboardEntry, kind: ContentKind) -> String {
    if has_label(e) {
        return e.label.as_deref().unwrap_or_default().trim().to_string();
    }
    match &e.content {
        ClipboardContent::Text(_) => subtitle(e),
        ClipboardContent::Image { .. } => kind.title().to_string(),
    }
}

/// Second line of a row: the content (when a label is the title), else the
/// note (✎), else what kind of text it is and how long.
pub fn row_subtitle(e: &ClipboardEntry, kind: ContentKind) -> String {
    if e.is_image() || has_label(e) {
        return subtitle(e);
    }
    if let Some(note) = e.note.as_deref().filter(|n| !n.trim().is_empty()) {
        let one_line = note.split_whitespace().collect::<Vec<_>>().join(" ");
        return format!("\u{270e} {}", truncate_chars(&one_line, SUBTITLE_MAX_CHARS));
    }
    let lines = match &e.content {
        ClipboardContent::Text(t) => t.trim_end().lines().count().max(1),
        ClipboardContent::Image { .. } => 1,
    };
    format!("{} \u{00b7} {} {}", kind.title(), lines, if lines == 1 { "line" } else { "lines" })
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        s.chars().take(max).collect::<String>() + "\u{2026}"
    } else {
        s.to_string()
    }
}

/// "1,234" — thousands separators for counts shown to the user.
fn group_thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn human_size(bytes: u64) -> String {
    match bytes {
        b if b >= 1024 * 1024 => format!("{:.1} MB", b as f64 / (1024.0 * 1024.0)),
        b if b >= 1024 => format!("{} KB", b / 1024),
        b => format!("{b} B"),
    }
}

/// Header line of the preview popover.
pub fn preview_header(e: &ClipboardEntry, file_size: Option<u64>) -> String {
    match &e.content {
        ClipboardContent::Text(t) => {
            let chars = t.chars().count();
            let lines = t.lines().count().max(1);
            format!(
                "Text \u{00b7} {} characters \u{00b7} {} {}",
                group_thousands(chars),
                group_thousands(lines),
                if lines == 1 { "line" } else { "lines" }
            )
        }
        ClipboardContent::Image { width, height, .. } => {
            let mut s = format!("Image \u{00b7} {width} \u{00d7} {height} \u{00b7} PNG");
            if let Some(size) = file_size {
                s.push_str(&format!(" \u{00b7} {}", human_size(size)));
            }
            s
        }
    }
}

/// Text for the preview (at most `max_chars`) and a note about the rest.
pub fn preview_body(text: &str, max_chars: usize) -> (String, Option<String>) {
    let total = text.chars().count();
    if total <= max_chars {
        return (text.to_string(), None);
    }
    let body: String = text.chars().take(max_chars).collect();
    let note = format!("\u{2026} {} more characters", group_thousands(total - max_chars));
    (body, Some(note))
}

/// Second line of a row: a one-line preview, or image dimensions.
pub fn subtitle(e: &ClipboardEntry) -> String {
    match &e.content {
        ClipboardContent::Text(t) => {
            let mut out = String::new();
            for word in t.split_whitespace() {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(word);
                if out.len() > SUBTITLE_MAX_CHARS * 4 {
                    break;
                }
            }
            if out.chars().count() > SUBTITLE_MAX_CHARS {
                out = out.chars().take(SUBTITLE_MAX_CHARS).collect::<String>() + "\u{2026}";
            }
            out
        }
        ClipboardContent::Image { width, height, .. } => {
            format!("{width} \u{00d7} {height} \u{2022} PNG")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::entry::ClipboardEntry;
    use crate::clipboard::kind::ContentKind;

    #[test]
    fn relative_time_boundaries() {
        let now = 1_000_000_000;
        let cases = [
            (0, "just now"),
            (9, "just now"),
            (10, "10 sec ago"),
            (59, "59 sec ago"),
            (60, "1 min ago"),
            (3599, "59 min ago"),
            (3600, "1 hour ago"),
            (7200, "2 hours ago"),
            (86_399, "23 hours ago"),
            (86_400, "yesterday"),
            (2 * 86_400, "2 days ago"),
            (29 * 86_400, "29 days ago"),
            (30 * 86_400, "1 month ago"),
            (169 * 86_400, "5 months ago"),
            (365 * 86_400, "1 year ago"),
            (800 * 86_400, "2 years ago"),
        ];
        for (ago, want) in cases {
            assert_eq!(relative_time(now - ago, now), want, "{ago}s ago");
        }
        // Clock went backwards / future timestamp.
        assert_eq!(relative_time(now + 50, now), "just now");
    }

    #[test]
    fn title_is_label_else_content() {
        let mut e = ClipboardEntry::new_text(1, "  https://x.y/a \n more".into());
        assert_eq!(title(&e, ContentKind::Url), "https://x.y/a more");
        e.label = Some("Docs".into());
        assert_eq!(title(&e, ContentKind::Url), "Docs");
        e.label = Some("  ".into());
        assert_eq!(title(&e, ContentKind::Url), "https://x.y/a more");
        let img = ClipboardEntry::new_image(2, [0; 32], 10, 10);
        assert_eq!(title(&img, ContentKind::Screenshot), "Screenshot");
    }

    #[test]
    fn row_subtitle_shows_content_note_or_meta() {
        let mut e = ClipboardEntry::new_text(1, "ssh deploy@host".into());
        assert_eq!(row_subtitle(&e, ContentKind::Shell), "Shell \u{00b7} 1 line");
        e.note = Some("needs VPN\npassword in vault".into());
        assert_eq!(row_subtitle(&e, ContentKind::Shell), "\u{270e} needs VPN password in vault");
        e.label = Some("Staging login".into());
        assert_eq!(row_subtitle(&e, ContentKind::Shell), "ssh deploy@host");
        let multi = ClipboardEntry::new_text(2, "a\nb\nc".into());
        assert_eq!(row_subtitle(&multi, ContentKind::Text), "Text \u{00b7} 3 lines");
        let img = ClipboardEntry::new_image(3, [0; 32], 1920, 1080);
        assert_eq!(row_subtitle(&img, ContentKind::Image), "1920 \u{00d7} 1080 \u{2022} PNG");
    }

    #[test]
    fn preview_header_describes_text_and_images() {
        let t = ClipboardEntry::new_text(1, "a\nb\nc".into());
        assert_eq!(preview_header(&t, None), "Text \u{00b7} 5 characters \u{00b7} 3 lines");
        let big = ClipboardEntry::new_text(2, "x".repeat(1234));
        assert_eq!(preview_header(&big, None), "Text \u{00b7} 1,234 characters \u{00b7} 1 line");
        let img = ClipboardEntry::new_image(3, [0; 32], 1920, 1080);
        assert_eq!(
            preview_header(&img, Some(1_258_291)),
            "Image \u{00b7} 1920 \u{00d7} 1080 \u{00b7} PNG \u{00b7} 1.2 MB"
        );
    }

    #[test]
    fn preview_body_is_truncated_with_a_note() {
        let (body, note) = preview_body("short", 10);
        assert_eq!((body.as_str(), note), ("short", None));
        let (body, note) = preview_body(&"y".repeat(25), 10);
        assert_eq!(body.chars().count(), 10);
        assert_eq!(note.as_deref(), Some("\u{2026} 15 more characters"));
    }

    #[test]
    fn subtitle_collapses_whitespace_and_describes_images() {
        let e = ClipboardEntry::new_text(1, "a\n\n  b\tc  ".into());
        assert_eq!(subtitle(&e), "a b c");
        let img = ClipboardEntry::new_image(2, [0; 32], 1920, 1080);
        assert_eq!(subtitle(&img), "1920 \u{00d7} 1080 \u{2022} PNG");
        let long = ClipboardEntry::new_text(3, "x".repeat(1000));
        assert!(subtitle(&long).chars().count() <= 201);
    }
}
