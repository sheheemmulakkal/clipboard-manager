use std::collections::VecDeque;

use crate::clipboard::entry::{now_secs, ClipboardContent, ClipboardEntry, EntryMeta};
use crate::store::Store;

pub struct MemoryStore {
    entries:     VecDeque<ClipboardEntry>,
    max_history: usize,
    deduplicate: bool,
    next_id:     u64,
}

impl MemoryStore {
    pub fn new(max_history: usize, deduplicate: bool) -> Self {
        Self {
            entries: VecDeque::new(),
            max_history,
            deduplicate,
            next_id: 1,
        }
    }
}

impl Store for MemoryStore {
    fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn add(&mut self, entry: ClipboardEntry) {
        self.next_id = self.next_id.max(entry.id + 1);
        if self.deduplicate {
            let existing = self.entries.iter().position(|e| same_content(&e.content, &entry.content));
            if let Some(pos) = existing {
                if let Some(mut old) = self.entries.remove(pos) {
                    old.copied_at = entry.copied_at.max(old.copied_at);
                    self.entries.push_back(old);
                }
                return;
            }
        }
        if self.entries.len() >= self.max_history {
            if let Some(pos) = self.entries.iter().position(|e| !e.pinned) {
                self.entries.remove(pos);
            } else {
                return; // all pinned, no room
            }
        }
        self.entries.push_back(entry);
    }

    fn set_pinned(&mut self, id: u64, pinned: bool) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.pinned = pinned;
        }
    }

    fn get(&self, id: u64) -> Option<&ClipboardEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    fn touch(&mut self, id: u64) {
        if let Some(pos) = self.entries.iter().position(|e| e.id == id) {
            if let Some(mut e) = self.entries.remove(pos) {
                e.copied_at = now_secs().max(e.copied_at);
                self.entries.push_back(e);
            }
        }
    }

    fn set_text(&mut self, id: u64, text: String) {
        if text.trim().is_empty() {
            return;
        }
        let is_text = matches!(self.get(id).map(|e| &e.content), Some(ClipboardContent::Text(_)));
        if !is_text {
            return;
        }
        if self.deduplicate {
            self.entries.retain(|e| {
                e.id == id || !matches!(&e.content, ClipboardContent::Text(t) if *t == text)
            });
        }
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.content = ClipboardContent::Text(text);
        }
    }

    fn set_meta(&mut self, id: u64, meta: EntryMeta) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.label = meta.label;
            e.color = meta.color;
            e.tag   = meta.tag;
        }
    }

    fn clear_unpinned(&mut self) {
        self.entries.retain(|e| e.pinned);
    }

    fn restore(&mut self, entries: Vec<ClipboardEntry>) {
        for e in entries {
            if !self.entries.iter().any(|x| x.id == e.id) {
                self.next_id = self.next_id.max(e.id + 1);
                self.entries.push_back(e);
            }
        }
        self.entries.make_contiguous().sort_by_key(|e| e.copied_at);
        while self.entries.len() > self.max_history {
            match self.entries.iter().position(|e| !e.pinned) {
                Some(pos) => { self.entries.remove(pos); }
                None => break,
            }
        }
    }

    fn get_all(&self) -> Vec<&ClipboardEntry> {
        self.entries.iter().collect()
    }

    fn remove(&mut self, id: u64) {
        self.entries.retain(|e| e.id != id);
    }

    fn clear(&mut self) {
        self.entries.clear();
    }

    fn contains_text(&self, text: &str) -> bool {
        self.entries.iter().any(|e| {
            if let ClipboardContent::Text(t) = &e.content { t == text } else { false }
        })
    }

    fn contains_image_hash(&self, hash: &[u8; 32]) -> bool {
        self.entries.iter().any(|e| {
            if let ClipboardContent::Image { hash: h, .. } = &e.content { h == hash } else { false }
        })
    }

    fn len(&self) -> usize {
        self.entries.len()
    }
}

fn same_content(a: &ClipboardContent, b: &ClipboardContent) -> bool {
    match (a, b) {
        (ClipboardContent::Text(x), ClipboardContent::Text(y)) => x == y,
        (ClipboardContent::Image { hash: x, .. }, ClipboardContent::Image { hash: y, .. }) => x == y,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_text(id: u64, content: &str) -> ClipboardEntry {
        ClipboardEntry::new_text(id, content.to_string())
    }

    #[test]
    fn test_add_respects_max_history() {
        let mut store = MemoryStore::new(3, false);
        store.add(make_text(1, "a"));
        store.add(make_text(2, "b"));
        store.add(make_text(3, "c"));
        store.add(make_text(4, "d"));
        assert_eq!(store.len(), 3);
        assert!(!store.contains_text("a"));
        assert!(store.contains_text("d"));
    }

    #[test]
    fn test_deduplication_skips_duplicates() {
        let mut store = MemoryStore::new(10, true);
        store.add(make_text(1, "hello"));
        store.add(make_text(2, "hello"));
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn test_remove_by_id() {
        let mut store = MemoryStore::new(10, false);
        store.add(make_text(1, "a"));
        store.add(make_text(2, "b"));
        store.remove(1);
        assert_eq!(store.len(), 1);
        assert!(store.contains_text("b"));
        assert!(!store.contains_text("a"));
    }

    #[test]
    fn next_id_is_monotonic_after_remove() {
        let mut store = MemoryStore::new(10, false);
        let a = store.next_id();
        store.add(make_text(a, "a"));
        let b = store.next_id();
        store.add(make_text(b, "b"));
        store.remove(b);
        let c = store.next_id();
        assert!(a < b && b < c);
    }

    #[test]
    fn next_id_starts_after_loaded_ids() {
        let mut store = MemoryStore::new(10, false);
        store.add(make_text(41, "loaded"));
        assert_eq!(store.next_id(), 42);
    }

    #[test]
    fn restore_brings_back_cleared_entries_in_age_order() {
        let mut store = MemoryStore::new(10, true);
        let mut a = make_text(1, "a"); a.copied_at = 10;
        let mut b = make_text(2, "b"); b.copied_at = 20;
        store.add(a);
        store.add(b);
        let snapshot: Vec<ClipboardEntry> = store.get_all().into_iter().cloned().collect();
        store.clear_unpinned();
        let mut c = make_text(3, "c"); c.copied_at = 30;
        store.add(c);
        store.restore(snapshot);
        let ids: Vec<u64> = store.get_all().iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![1, 2, 3]);
    }

    #[test]
    fn restore_respects_max_history_and_skips_present() {
        let mut store = MemoryStore::new(2, true);
        let mut a = make_text(1, "a"); a.copied_at = 10;
        let mut b = make_text(2, "b"); b.copied_at = 20;
        store.add(a.clone());
        store.add(b.clone());
        store.clear_unpinned();
        let mut c = make_text(3, "c"); c.copied_at = 30;
        store.add(c.clone());
        b.copied_at = 20;
        store.restore(vec![a, b, c]);
        let ids: Vec<u64> = store.get_all().iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![2, 3]); // oldest (1) evicted, 3 not duplicated
    }

    #[test]
    fn recopy_moves_to_top_and_keeps_meta() {
        let mut s = MemoryStore::new(10, true);
        let mut a = make_text(1, "a");
        a.copied_at = 100;
        a.label = Some("L".into());
        a.color = Some("red".into());
        a.pinned = true;
        s.add(a);
        let mut b = make_text(2, "b");
        b.copied_at = 200;
        s.add(b);
        let mut again = make_text(3, "a");
        again.copied_at = 300;
        s.add(again);
        let all = s.get_all();
        assert_eq!(all.len(), 2);
        let last = all.last().unwrap();
        assert_eq!(last.id, 1);
        assert_eq!(last.copied_at, 300);
        assert!(last.pinned);
        assert_eq!(last.label.as_deref(), Some("L"));
        assert_eq!(last.color.as_deref(), Some("red"));
    }

    #[test]
    fn recopy_image_moves_to_top() {
        let mut s = MemoryStore::new(10, true);
        let mut img = ClipboardEntry::new_image(1, [9; 32], 4, 4);
        img.copied_at = 100;
        s.add(img);
        let mut t = make_text(2, "t");
        t.copied_at = 200;
        s.add(t);
        let mut again = ClipboardEntry::new_image(3, [9; 32], 4, 4);
        again.copied_at = 300;
        s.add(again);
        let ids: Vec<u64> = s.get_all().iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![2, 1]);
        assert_eq!(s.get(1).unwrap().copied_at, 300);
    }

    #[test]
    fn touch_moves_to_top() {
        let mut s = MemoryStore::new(10, true);
        let mut a = make_text(1, "a");
        a.copied_at = 100;
        s.add(a);
        s.add(make_text(2, "b"));
        s.touch(1);
        let all = s.get_all();
        assert_eq!(all.last().unwrap().id, 1);
        assert!(all.last().unwrap().copied_at > 100);
    }

    #[test]
    fn dedup_disabled_keeps_duplicates() {
        let mut s = MemoryStore::new(10, false);
        s.add(make_text(1, "a"));
        s.add(make_text(2, "a"));
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn recopy_at_capacity_does_not_evict() {
        let mut s = MemoryStore::new(2, true);
        s.add(make_text(1, "a"));
        s.add(make_text(2, "b"));
        s.add(make_text(3, "a"));
        let ids: Vec<u64> = s.get_all().iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![2, 1]);
    }

    #[test]
    fn set_text_edits_content() {
        let mut s = MemoryStore::new(10, true);
        s.add(make_text(1, "old"));
        s.set_text(1, "new".into());
        assert!(s.contains_text("new"));
        assert!(!s.contains_text("old"));
    }

    #[test]
    fn set_text_ignores_empty_and_images() {
        let mut s = MemoryStore::new(10, true);
        s.add(make_text(1, "keep"));
        s.add(ClipboardEntry::new_image(2, [1; 32], 2, 2));
        s.set_text(1, "   ".into());
        s.set_text(2, "text".into());
        assert!(s.contains_text("keep"));
        assert!(s.get(2).unwrap().is_image());
    }

    #[test]
    fn set_text_to_existing_text_merges_duplicates() {
        let mut s = MemoryStore::new(10, true);
        s.add(make_text(1, "a"));
        s.add(make_text(2, "b"));
        s.set_text(1, "b".into());
        let ids: Vec<u64> = s.get_all().iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![1]);
    }

    #[test]
    fn test_contains_text() {
        let mut store = MemoryStore::new(10, false);
        store.add(make_text(1, "hello world"));
        assert!(store.contains_text("hello world"));
        assert!(!store.contains_text("goodbye"));
    }
}
