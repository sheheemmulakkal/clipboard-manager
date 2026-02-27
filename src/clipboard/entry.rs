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

#[derive(Debug, Clone)]
pub struct ClipboardEntry {
    pub id:         u64,
    pub content:    ClipboardContent,
    pub copied_at:  u64,
    pub pinned:     bool,
    pub label:      Option<String>,
    pub color:      Option<String>,
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
        }
    }

    pub fn preview(&self) -> String {
        match &self.content {
            ClipboardContent::Text(t) => {
                let end = t.char_indices().nth(80).map(|(i, _)| i).unwrap_or(t.len());
                t[..end].to_string()
            }
            ClipboardContent::Image { width, height, .. } => {
                format!("Image {width}\u{00d7}{height}")
            }
        }
    }

    #[allow(dead_code)]
    pub fn as_text(&self) -> Option<&str> {
        if let ClipboardContent::Text(t) = &self.content { Some(t) } else { None }
    }

    pub fn is_image(&self) -> bool {
        matches!(&self.content, ClipboardContent::Image { .. })
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
