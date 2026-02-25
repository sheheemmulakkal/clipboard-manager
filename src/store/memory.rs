use std::collections::VecDeque;

use crate::clipboard::entry::ClipboardEntry;
use crate::store::Store;

pub struct MemoryStore {
    entries: VecDeque<ClipboardEntry>,
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
        if self.deduplicate && self.contains_content(&entry.content) {
            return;
        }
        if self.entries.len() >= self.max_history {
            // Evict the oldest non-pinned entry
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

    fn contains_content(&self, content: &str) -> bool {
        self.entries.iter().any(|e| e.content == content)
    }

    fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(id: u64, content: &str) -> ClipboardEntry {
        ClipboardEntry::new(id, content.to_string())
    }

    #[test]
    fn test_add_respects_max_history() {
        let mut store = MemoryStore::new(3, false);
        store.add(make_entry(1, "a"));
        store.add(make_entry(2, "b"));
        store.add(make_entry(3, "c"));
        store.add(make_entry(4, "d"));
        assert_eq!(store.len(), 3);
        assert!(!store.contains_content("a"));
        assert!(store.contains_content("d"));
    }

    #[test]
    fn test_deduplication_skips_duplicates() {
        let mut store = MemoryStore::new(10, true);
        store.add(make_entry(1, "hello"));
        store.add(make_entry(2, "hello"));
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn test_remove_by_id() {
        let mut store = MemoryStore::new(10, false);
        store.add(make_entry(1, "a"));
        store.add(make_entry(2, "b"));
        store.remove(1);
        assert_eq!(store.len(), 1);
        assert!(store.contains_content("b"));
        assert!(!store.contains_content("a"));
    }

    #[test]
    fn test_contains_content() {
        let mut store = MemoryStore::new(10, false);
        store.add(make_entry(1, "hello world"));
        assert!(store.contains_content("hello world"));
        assert!(!store.contains_content("goodbye"));
    }
}
