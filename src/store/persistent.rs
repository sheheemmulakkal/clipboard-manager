use std::path::PathBuf;

use crate::clipboard::entry::ClipboardEntry;
use crate::store::engine::PersistenceEngine;
use crate::store::memory::MemoryStore;
use crate::store::Store;

/// Decorator around `MemoryStore` that persists every mutation to disk.
/// All business logic (eviction, dedup, max_history) stays in `MemoryStore`.
pub struct PersistentStore {
    inner:  MemoryStore,
    engine: PersistenceEngine,
}

impl PersistentStore {
    /// Load history from `path` (if it exists) and initialise the in-memory store.
    /// Entries that exceed `max_history` are naturally evicted by `MemoryStore::add`.
    pub fn load(max_history: usize, deduplicate: bool, path: PathBuf) -> Self {
        let engine = PersistenceEngine::new(path);
        let mut inner = MemoryStore::new(max_history, deduplicate);
        let loaded = engine.load();
        tracing::debug!("[persist] loaded {} entries from disk", loaded.len());
        for entry in loaded {
            inner.add(entry);
        }
        Self { inner, engine }
    }

    fn flush(&self) {
        let entries: Vec<&ClipboardEntry> = self.inner.get_all();
        if let Err(e) = self.engine.flush(&entries) {
            tracing::warn!("[persist] flush failed: {e}");
        }
    }
}

impl Store for PersistentStore {
    fn add(&mut self, entry: ClipboardEntry) {
        self.inner.add(entry);
        self.flush();
    }

    fn remove(&mut self, id: u64) {
        self.inner.remove(id);
        self.flush();
    }

    fn set_pinned(&mut self, id: u64, pinned: bool) {
        self.inner.set_pinned(id, pinned);
        self.flush();
    }

    fn set_label(&mut self, id: u64, label: Option<String>, color: Option<String>) {
        self.inner.set_label(id, label, color);
        self.flush();
    }

    fn clear_unpinned(&mut self) {
        self.inner.clear_unpinned();
        self.flush();
    }

    fn clear(&mut self) {
        self.inner.clear();
        self.flush();
    }

    fn get_all(&self) -> Vec<&ClipboardEntry> {
        self.inner.get_all()
    }

    fn len(&self) -> usize {
        self.inner.len()
    }

    fn contains_content(&self, content: &str) -> bool {
        self.inner.contains_content(content)
    }
}
