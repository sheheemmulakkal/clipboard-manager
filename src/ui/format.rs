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

/// Bold first line of a row: the user's label, or the kind of content.
pub fn title(e: &ClipboardEntry, kind: ContentKind) -> String {
    match e.label.as_deref().map(str::trim) {
        Some(l) if !l.is_empty() => l.to_string(),
        _ => kind.title().to_string(),
    }
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
    fn title_prefers_label_then_kind() {
        let mut e = ClipboardEntry::new_text(1, "https://x.y".into());
        assert_eq!(title(&e, ContentKind::Url), "URL");
        e.label = Some("Docs".into());
        assert_eq!(title(&e, ContentKind::Url), "Docs");
        e.label = Some("  ".into());
        assert_eq!(title(&e, ContentKind::Url), "URL");
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
