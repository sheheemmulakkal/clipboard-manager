use std::collections::VecDeque;

use crate::clipboard::entry::{ClipboardContent, ClipboardEntry};
use crate::store::Store;

pub struct MemoryStore {
    entries:     VecDeque<ClipboardEntry>,
    max_history: usize,
    deduplicate: bool,
}

impl MemoryStore {
    pub fn new(max_history: usize, deduplicate: bool) -> Self {
        Self {
            entries: VecDeque::new(),
            max_history,
            deduplicate,
        }
    }
}

impl Store for MemoryStore {
    fn add(&mut self, entry: ClipboardEntry) {
        if self.deduplicate {
            let dupe = match &entry.content {
                ClipboardContent::Text(t) => self.contains_text(t),
                ClipboardContent::Image { hash, .. } => self.contains_image_hash(hash),
            };
            if dupe { return; }
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

    fn set_label(&mut self, id: u64, label: Option<String>, color: Option<String>) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.label = label;
            e.color = color;
        }
    }

    fn clear_unpinned(&mut self) {
        self.entries.retain(|e| e.pinned);
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
    fn test_contains_text() {
        let mut store = MemoryStore::new(10, false);
        store.add(make_text(1, "hello world"));
        assert!(store.contains_text("hello world"));
        assert!(!store.contains_text("goodbye"));
    }
}
