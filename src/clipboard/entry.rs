use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ClipboardEntry {
    pub id: u64,
    pub content: String,
    pub copied_at: u64,
    pub pinned: bool,
    pub label: Option<String>,
}

impl ClipboardEntry {
    pub fn new(id: u64, content: String) -> Self {
        let copied_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            id,
            content,
            copied_at,
            pinned: false,
            label: None,
        }
    }

    #[allow(dead_code)]
    pub fn preview(&self) -> &str {
        let end = self
            .content
            .char_indices()
            .nth(80)
            .map(|(i, _)| i)
            .unwrap_or(self.content.len());
        &self.content[..end]
    }
}
