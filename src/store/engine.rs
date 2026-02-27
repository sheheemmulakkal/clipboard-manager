use std::io::Write;
use std::path::PathBuf;

use crate::clipboard::entry::{ClipboardContent, ClipboardEntry};

const MAGIC: &[u8; 8] = b"CLIPMGR1";
const VERSION: u16 = 3;
const MAX_ENTRY_BYTES: u32 = 10 * 1024 * 1024; // 10 MB guard

pub struct PersistenceEngine {
    path: PathBuf,
}

impl PersistenceEngine {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Load entries from disk. Returns empty vec on any error (fail-safe).
    pub fn load(&self) -> Vec<ClipboardEntry> {
        match std::fs::read(&self.path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
            Err(e) => {
                tracing::warn!("[persist] read error: {e}");
                vec![]
            }
            Ok(data) => parse_file(&data),
        }
    }

    /// Atomically write all entries to disk via a .tmp + rename.
    pub fn flush(&self, entries: &[&ClipboardEntry]) -> anyhow::Result<()> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("history file has no parent directory"))?;
        std::fs::create_dir_all(parent)?;

        let file_name = self
            .path
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("history file path has no file name"))?
            .to_string_lossy();
        let tmp = parent.join(format!("{file_name}.tmp"));

        let write_result: anyhow::Result<()> = (|| {
            let mut f = std::fs::File::create(&tmp)?;
            // Header: magic(8) + version(2) + flags(2) + count(4) + reserved(6) = 22 bytes
            f.write_all(MAGIC)?;
            f.write_all(&VERSION.to_le_bytes())?;
            f.write_all(&0u16.to_le_bytes())?; // flags
            f.write_all(&(entries.len() as u32).to_le_bytes())?;
            f.write_all(&[0u8; 6])?; // reserved
            for e in entries {
                write_entry(&mut f, e)?;
            }
            f.flush()?;
            Ok(())
        })();

        if write_result.is_err() {
            let _ = std::fs::remove_file(&tmp);
            return write_result;
        }

        Ok(std::fs::rename(&tmp, &self.path)?)
    }
}

// ── CRC32-IEEE (inline, no external dependency) ──────────────────────────────

fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

// ── Entry serialization (V3 format) ───────────────────────────────────────────
//
// V3 text entry:
//   type(1)=0 | id(8) | copied_at(8) | pinned(1) | pad(3) | content_len(4) | content(n)
//   | has_label(1) | [label_len(4) | label(n)]
//   | has_color(1) | [color_len(4) | color(n)]
//   | crc32(4)   ← CRC covers from id(8) onward
//
// V3 image entry:
//   type(1)=1 | id(8) | copied_at(8) | pinned(1) | pad(3) | hash(32) | width(4) | height(4)
//   | has_label(1) | [label_len(4) | label(n)]
//   | has_color(1) | [color_len(4) | color(n)]
//   | crc32(4)   ← CRC covers from id(8) onward

fn write_entry(w: &mut impl Write, e: &ClipboardEntry) -> anyhow::Result<()> {
    match &e.content {
        ClipboardContent::Text(text) => {
            w.write_all(&[0u8])?; // type = 0 (text)
            let content_bytes = text.as_bytes();
            let mut buf: Vec<u8> = Vec::with_capacity(32 + content_bytes.len());
            buf.extend_from_slice(&e.id.to_le_bytes());
            buf.extend_from_slice(&e.copied_at.to_le_bytes());
            buf.push(e.pinned as u8);
            buf.extend_from_slice(&[0u8; 3]); // pad
            buf.extend_from_slice(&(content_bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(content_bytes);
            write_label_color(&mut buf, &e.label, &e.color);
            let checksum = crc32(&buf);
            buf.extend_from_slice(&checksum.to_le_bytes());
            w.write_all(&buf)?;
        }
        ClipboardContent::Image { hash, width, height } => {
            w.write_all(&[1u8])?; // type = 1 (image)
            let mut buf: Vec<u8> = Vec::with_capacity(64);
            buf.extend_from_slice(&e.id.to_le_bytes());
            buf.extend_from_slice(&e.copied_at.to_le_bytes());
            buf.push(e.pinned as u8);
            buf.extend_from_slice(&[0u8; 3]); // pad
            buf.extend_from_slice(hash);       // 32 bytes
            buf.extend_from_slice(&width.to_le_bytes());
            buf.extend_from_slice(&height.to_le_bytes());
            write_label_color(&mut buf, &e.label, &e.color);
            let checksum = crc32(&buf);
            buf.extend_from_slice(&checksum.to_le_bytes());
            w.write_all(&buf)?;
        }
    }
    Ok(())
}

fn write_label_color(buf: &mut Vec<u8>, label: &Option<String>, color: &Option<String>) {
    match label {
        Some(lbl) => {
            let lb = lbl.as_bytes();
            buf.push(1u8);
            buf.extend_from_slice(&(lb.len() as u32).to_le_bytes());
            buf.extend_from_slice(lb);
        }
        None => buf.push(0u8),
    }
    match color {
        Some(col) => {
            let cb = col.as_bytes();
            buf.push(1u8);
            buf.extend_from_slice(&(cb.len() as u32).to_le_bytes());
            buf.extend_from_slice(cb);
        }
        None => buf.push(0u8),
    }
}

// ── File parsing ──────────────────────────────────────────────────────────────

fn parse_file(data: &[u8]) -> Vec<ClipboardEntry> {
    if data.len() < 22 {
        tracing::warn!("[persist] file too short — ignoring");
        return vec![];
    }
    if &data[0..8] != MAGIC {
        tracing::warn!("[persist] bad magic bytes — ignoring history file");
        return vec![];
    }
    let version = u16::from_le_bytes([data[8], data[9]]);
    if version != 1 && version != 2 && version != 3 {
        tracing::warn!("[persist] unsupported file version {version} — ignoring history file");
        return vec![];
    }
    let count = u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as usize;

    let mut pos = 22usize;
    let mut entries = Vec::with_capacity(count.min(1024));

    for i in 0..count {
        let result = match version {
            1 => read_entry_v1(data, &mut pos),
            2 => read_entry_v2(data, &mut pos),
            _ => read_entry_v3(data, &mut pos),
        };
        match result {
            Some(e) => entries.push(e),
            None => {
                tracing::warn!(
                    "[persist] corrupt/truncated at entry {i} — recovered {}/{count} entries",
                    entries.len()
                );
                break;
            }
        }
    }

    entries
}

// ── Shared read macros helper ─────────────────────────────────────────────────

macro_rules! try_read_bytes {
    ($data:expr, $pos:expr, $n:expr) => {{
        let end = *$pos + $n;
        if end > $data.len() {
            return None;
        }
        let s = &$data[*$pos..end];
        *$pos = end;
        s
    }};
}

macro_rules! try_read_u8 {
    ($data:expr, $pos:expr) => {
        try_read_bytes!($data, $pos, 1)[0]
    };
}

macro_rules! try_read_u32 {
    ($data:expr, $pos:expr) => {{
        let b = try_read_bytes!($data, $pos, 4);
        u32::from_le_bytes([b[0], b[1], b[2], b[3]])
    }};
}

macro_rules! try_read_u64 {
    ($data:expr, $pos:expr) => {{
        let b = try_read_bytes!($data, $pos, 8);
        u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
    }};
}

/// Read a V1 entry (no color field). Sets color = None.
fn read_entry_v1(data: &[u8], pos: &mut usize) -> Option<ClipboardEntry> {
    let entry_start = *pos;

    let id        = try_read_u64!(data, pos);
    let copied_at = try_read_u64!(data, pos);
    let pinned    = try_read_u8!(data, pos) != 0;
    let _         = try_read_bytes!(data, pos, 3); // pad

    let content_len = try_read_u32!(data, pos);
    if content_len > MAX_ENTRY_BYTES {
        tracing::warn!("[persist] entry content too large ({content_len} bytes) — skipping");
        return None;
    }
    let content_bytes = try_read_bytes!(data, pos, content_len as usize);
    let text = match std::str::from_utf8(content_bytes) {
        Ok(s) => s.to_string(),
        Err(_) => {
            tracing::warn!("[persist] invalid UTF-8 in content — skipping entry");
            return None;
        }
    };

    let has_label = try_read_u8!(data, pos);
    let label = if has_label == 1 {
        let label_len = try_read_u32!(data, pos);
        if label_len > MAX_ENTRY_BYTES {
            tracing::warn!("[persist] label too large ({label_len} bytes) — skipping");
            return None;
        }
        let label_bytes = try_read_bytes!(data, pos, label_len as usize);
        match std::str::from_utf8(label_bytes) {
            Ok(s) => Some(s.to_string()),
            Err(_) => {
                tracing::warn!("[persist] invalid UTF-8 in label — skipping entry");
                return None;
            }
        }
    } else {
        None
    };

    let expected = crc32(&data[entry_start..*pos]);
    let stored   = try_read_u32!(data, pos);
    if stored != expected {
        tracing::warn!(
            "[persist] CRC32 mismatch (expected {expected:#010x}, got {stored:#010x}) — skipping entry"
        );
        return None;
    }

    Some(ClipboardEntry {
        id,
        content: ClipboardContent::Text(text),
        copied_at,
        pinned,
        label,
        color: None,
    })
}

/// Read a V2 entry (includes color field after label).
fn read_entry_v2(data: &[u8], pos: &mut usize) -> Option<ClipboardEntry> {
    let entry_start = *pos;

    let id        = try_read_u64!(data, pos);
    let copied_at = try_read_u64!(data, pos);
    let pinned    = try_read_u8!(data, pos) != 0;
    let _         = try_read_bytes!(data, pos, 3); // pad

    let content_len = try_read_u32!(data, pos);
    if content_len > MAX_ENTRY_BYTES {
        tracing::warn!("[persist] entry content too large ({content_len} bytes) — skipping");
        return None;
    }
    let content_bytes = try_read_bytes!(data, pos, content_len as usize);
    let text = match std::str::from_utf8(content_bytes) {
        Ok(s) => s.to_string(),
        Err(_) => {
            tracing::warn!("[persist] invalid UTF-8 in content — skipping entry");
            return None;
        }
    };

    let (label, color) = read_label_color(data, pos)?;

    let expected = crc32(&data[entry_start..*pos]);
    let stored   = try_read_u32!(data, pos);
    if stored != expected {
        tracing::warn!(
            "[persist] CRC32 mismatch (expected {expected:#010x}, got {stored:#010x}) — skipping entry"
        );
        return None;
    }

    Some(ClipboardEntry {
        id,
        content: ClipboardContent::Text(text),
        copied_at,
        pinned,
        label,
        color,
    })
}

/// Read a V3 entry: reads type byte first, then dispatches text or image layout.
fn read_entry_v3(data: &[u8], pos: &mut usize) -> Option<ClipboardEntry> {
    if *pos >= data.len() {
        return None;
    }
    let entry_type = data[*pos];
    *pos += 1;

    let entry_start = *pos;

    let id        = try_read_u64!(data, pos);
    let copied_at = try_read_u64!(data, pos);
    let pinned    = try_read_u8!(data, pos) != 0;
    let _         = try_read_bytes!(data, pos, 3); // pad

    let content = if entry_type == 0 {
        // Text entry
        let content_len = try_read_u32!(data, pos);
        if content_len > MAX_ENTRY_BYTES {
            tracing::warn!("[persist] entry content too large ({content_len} bytes) — skipping");
            return None;
        }
        let content_bytes = try_read_bytes!(data, pos, content_len as usize);
        let text = match std::str::from_utf8(content_bytes) {
            Ok(s) => s.to_string(),
            Err(_) => {
                tracing::warn!("[persist] invalid UTF-8 in content — skipping entry");
                return None;
            }
        };
        ClipboardContent::Text(text)
    } else {
        // Image entry (type = 1)
        let hash_bytes = try_read_bytes!(data, pos, 32);
        let mut hash = [0u8; 32];
        hash.copy_from_slice(hash_bytes);
        let width  = try_read_u32!(data, pos);
        let height = try_read_u32!(data, pos);
        ClipboardContent::Image { hash, width, height }
    };

    let (label, color) = read_label_color(data, pos)?;

    let expected = crc32(&data[entry_start..*pos]);
    let stored   = try_read_u32!(data, pos);
    if stored != expected {
        tracing::warn!(
            "[persist] CRC32 mismatch (expected {expected:#010x}, got {stored:#010x}) — skipping entry"
        );
        return None;
    }

    Some(ClipboardEntry { id, content, copied_at, pinned, label, color })
}

/// Read label + color fields (shared by V2 and V3).
fn read_label_color(data: &[u8], pos: &mut usize) -> Option<(Option<String>, Option<String>)> {
    let has_label = try_read_u8!(data, pos);
    let label = if has_label == 1 {
        let label_len = try_read_u32!(data, pos);
        if label_len > MAX_ENTRY_BYTES {
            tracing::warn!("[persist] label too large ({label_len} bytes) — skipping");
            return None;
        }
        let label_bytes = try_read_bytes!(data, pos, label_len as usize);
        match std::str::from_utf8(label_bytes) {
            Ok(s) => Some(s.to_string()),
            Err(_) => {
                tracing::warn!("[persist] invalid UTF-8 in label — skipping entry");
                return None;
            }
        }
    } else {
        None
    };

    let has_color = try_read_u8!(data, pos);
    let color = if has_color == 1 {
        let color_len = try_read_u32!(data, pos);
        if color_len > MAX_ENTRY_BYTES {
            tracing::warn!("[persist] color too large ({color_len} bytes) — skipping");
            return None;
        }
        let color_bytes = try_read_bytes!(data, pos, color_len as usize);
        match std::str::from_utf8(color_bytes) {
            Ok(s) => Some(s.to_string()),
            Err(_) => {
                tracing::warn!("[persist] invalid UTF-8 in color — skipping entry");
                return None;
            }
        }
    } else {
        None
    };

    Some((label, color))
}
