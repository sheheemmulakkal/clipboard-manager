pub mod memory;

#[cfg(feature = "persist")]
pub mod engine;
#[cfg(feature = "persist")]
pub mod persistent;

use crate::clipboard::entry::{ClipboardEntry, EntryMeta};

#[allow(dead_code)]
pub trait Store: Send + Sync {
    /// Add a new entry. With deduplication on, re-adding content that is
    /// already stored moves the existing entry to the newest position
    /// (refreshing `copied_at`) and keeps its id, pin and metadata.
    fn add(&mut self, entry: ClipboardEntry);
    fn get(&self, id: u64) -> Option<&ClipboardEntry>;
    /// Mark an entry as just used: move it to the newest position.
    fn touch(&mut self, id: u64);
    /// Allocate a fresh id (monotonic; never reuses an id seen by this store).
    fn next_id(&mut self) -> u64;
    fn get_all(&self) -> Vec<&ClipboardEntry>;
    fn remove(&mut self, id: u64);
    fn clear(&mut self);
    fn contains_text(&self, text: &str) -> bool;
    fn contains_image_hash(&self, hash: &[u8; 32]) -> bool;
    fn len(&self) -> usize;
    fn set_pinned(&mut self, id: u64, pinned: bool);
    fn set_meta(&mut self, id: u64, meta: EntryMeta);
    /// Replace the text of a text entry (user edit). Empty text is ignored;
    /// with deduplication on, another entry with the same text is removed.
    fn set_text(&mut self, id: u64, text: String);
    /// Remove all entries that are not pinned.
    fn clear_unpinned(&mut self);
    /// Put previously removed entries back (undo). Entries whose id is
    /// already present are ignored; history stays ordered by age and
    /// within `max_history`.
    fn restore(&mut self, entries: Vec<ClipboardEntry>);
    /// Remove unpinned entries copied before `cutoff` (unix seconds).
    /// Returns how many were removed.
    fn expire_older_than(&mut self, cutoff: u64) -> usize;
}
