use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub enum ClipboardContent {
    Text(String),
    Image { hash: [u8; 32], width: u32, height: u32 },
}

impl Default for ClipboardContent {
    fn default() -> Self {
        ClipboardContent::Text(String::new())
    }
}

/// User-editable metadata of an entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntryMeta {
    pub label: Option<String>,
    pub color: Option<String>,
    pub tag:   Option<String>,
    /// Free-form text the user attached (searchable).
    pub note:  Option<String>,
}

#[derive(Debug, Clone)]
pub struct ClipboardEntry {
    pub id:         u64,
    pub content:    ClipboardContent,
    pub copied_at:  u64,
    pub pinned:     bool,
    pub label:      Option<String>,
    pub color:      Option<String>,
    pub tag:        Option<String>,
    pub note:       Option<String>,
}

impl ClipboardEntry {
    pub fn new_text(id: u64, text: String) -> Self {
        Self {
            id,
            content: ClipboardContent::Text(text),
            copied_at: now_secs(),
            pinned: false,
            label: None,
            color: None,
            tag: None,
            note: None,
        }
    }

    pub fn new_image(id: u64, hash: [u8; 32], width: u32, height: u32) -> Self {
        Self {
            id,
            content: ClipboardContent::Image { hash, width, height },
            copied_at: now_secs(),
            pinned: false,
            label: None,
            color: None,
            tag: None,
            note: None,
        }
    }

    pub fn is_image(&self) -> bool {
        matches!(&self.content, ClipboardContent::Image { .. })
    }
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
