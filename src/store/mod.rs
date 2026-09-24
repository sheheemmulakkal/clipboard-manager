pub mod memory;

#[cfg(feature = "persist")]
pub mod engine;
#[cfg(feature = "persist")]
pub mod persistent;

use crate::clipboard::entry::ClipboardEntry;

#[allow(dead_code)]
pub trait Store: Send + Sync {
    fn add(&mut self, entry: ClipboardEntry);
    /// Allocate a fresh id (monotonic; never reuses an id seen by this store).
    fn next_id(&mut self) -> u64;
    fn get_all(&self) -> Vec<&ClipboardEntry>;
    fn remove(&mut self, id: u64);
    fn clear(&mut self);
    fn contains_text(&self, text: &str) -> bool;
    fn contains_image_hash(&self, hash: &[u8; 32]) -> bool;
    fn len(&self) -> usize;
    fn set_pinned(&mut self, id: u64, pinned: bool);
    fn set_label(&mut self, id: u64, label: Option<String>, color: Option<String>);
    /// Remove all entries that are not pinned.
    fn clear_unpinned(&mut self);
}
