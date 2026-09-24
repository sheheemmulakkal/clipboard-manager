//! Ordering and search filtering of history entries (pure, no GTK).

use crate::clipboard::entry::{ClipboardContent, ClipboardEntry};
use crate::clipboard::kind::ContentKind;

/// Pinned entries first, then everything else; newest first within each group.
/// `entries` is in store order (oldest → newest), which breaks ties between
/// entries copied within the same second.
pub fn sorted(mut entries: Vec<ClipboardEntry>) -> Vec<ClipboardEntry> {
    entries.reverse();
    entries.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.copied_at.cmp(&a.copied_at)));
    entries
}

/// Case-insensitive substring match on content (or "image WxH"), detected
/// kind ("url", "code", …), label and tag.
/// An empty query matches everything.
pub fn matches_query(e: &ClipboardEntry, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let q = query.to_lowercase();
    let content_match = match &e.content {
        ClipboardContent::Text(t) => {
            t.to_lowercase().contains(&q)
                || ContentKind::detect(t).title().to_lowercase().contains(&q)
        }
        ClipboardContent::Image { width, height, .. } => {
            format!("image {width}\u{00d7}{height} {width}x{height}").contains(&q)
        }
    };
    content_match
        || e.label.as_deref().is_some_and(|l| l.to_lowercase().contains(&q))
        || e.tag.as_deref().is_some_and(|t| t.to_lowercase().contains(&q))
}

/// The entries the popup should show for `query`, in display order.
pub fn visible(all: Vec<ClipboardEntry>, query: &str) -> Vec<ClipboardEntry> {
    sorted(all).into_iter().filter(|e| matches_query(e, query)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(id: u64, s: &str, at: u64) -> ClipboardEntry {
        let mut e = ClipboardEntry::new_text(id, s.into());
        e.copied_at = at;
        e
    }

    #[test]
    fn pinned_first_then_newest() {
        let mut pinned = text(2, "p", 50);
        pinned.pinned = true;
        let got = sorted(vec![text(1, "old", 10), pinned, text(3, "new", 99)]);
        let ids: Vec<u64> = got.iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![2, 3, 1]);
    }

    #[test]
    fn same_second_keeps_newest_first() {
        // Store order is oldest → newest; copied_at has 1 s resolution.
        let got = sorted(vec![text(1, "a", 5), text(2, "b", 5), text(3, "c", 5)]);
        let ids: Vec<u64> = got.iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![3, 2, 1]);
    }

    #[test]
    fn search_is_case_insensitive_on_content_and_label() {
        let mut e = text(1, "some body", 1);
        e.label = Some("Resume".into());
        assert!(matches_query(&e, "res"));
        assert!(matches_query(&e, "BODY"));
        assert!(!matches_query(&e, "nothing"));
        assert!(matches_query(&e, ""));
    }

    #[test]
    fn search_matches_tag() {
        let mut e = text(1, "abc", 1);
        e.tag = Some("Work".into());
        assert!(matches_query(&e, "work"));
    }

    #[test]
    fn search_matches_kind_name() {
        assert!(matches_query(&text(1, "https://example.com", 1), "url"));
        assert!(matches_query(&text(2, "git status", 1), "shell"));
        assert!(!matches_query(&text(3, "plain words", 1), "url"));
    }

    #[test]
    fn search_matches_image_dimensions() {
        let e = ClipboardEntry::new_image(1, [0; 32], 1920, 1080);
        assert!(matches_query(&e, "1920"));
        assert!(matches_query(&e, "image"));
        assert!(!matches_query(&e, "1366"));
    }
}
