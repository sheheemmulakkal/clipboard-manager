pub mod memory;

use crate::clipboard::entry::ClipboardEntry;

#[allow(dead_code)]
pub trait Store: Send + Sync {
    fn add(&mut self, entry: ClipboardEntry);
    fn get_all(&self) -> Vec<&ClipboardEntry>;
    fn remove(&mut self, id: u64);
    fn clear(&mut self);
    fn contains_content(&self, content: &str) -> bool;
    fn len(&self) -> usize;
}
