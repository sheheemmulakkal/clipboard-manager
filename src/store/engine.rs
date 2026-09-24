use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;

use crate::clipboard::entry::{ClipboardContent, ClipboardEntry};

const MAGIC: &[u8; 8] = b"CLIPMGR1";
const VERSION: u16 = 4;
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
            Ok(data) => {
                make_private(&self.path);
                parse_file(&data)
            }
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
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp)?;
            // An existing .tmp keeps its old mode; force it.
            f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
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
            f.sync_all()?;
            Ok(())
        })();

        if write_result.is_err() {
            let _ = std::fs::remove_file(&tmp);
            return write_result;
        }

        Ok(std::fs::rename(&tmp, &self.path)?)
    }
}

/// Ensure `path` is readable only by the owner (history can hold secrets).
fn make_private(path: &std::path::Path) {
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.permissions().mode() & 0o077 != 0 {
            if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
                tracing::warn!("[persist] cannot restrict permissions: {e}");
            }
        }
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

// ── Entry serialization (V4 format) ───────────────────────────────────────────
//
// V4 = V3 + `| has_tag(1) | [tag_len(4) | tag(n)]` after the color, before the CRC.
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
            write_opt_string(&mut buf, &e.tag);
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
            write_opt_string(&mut buf, &e.tag);
            let checksum = crc32(&buf);
            buf.extend_from_slice(&checksum.to_le_bytes());
            w.write_all(&buf)?;
        }
    }
    Ok(())
}

fn write_label_color(buf: &mut Vec<u8>, label: &Option<String>, color: &Option<String>) {
    write_opt_string(buf, label);
    write_opt_string(buf, color);
}

fn write_opt_string(buf: &mut Vec<u8>, value: &Option<String>) {
    match value {
        Some(v) => {
            buf.push(1u8);
            buf.extend_from_slice(&(v.len() as u32).to_le_bytes());
            buf.extend_from_slice(v.as_bytes());
        }
        None => buf.push(0u8),
    }
}

// ── File parsing ──────────────────────────────────────────────────────────────

/// Result of reading one entry.
enum ReadOutcome {
    Entry(ClipboardEntry),
    /// Entry was well-formed but rejected (oversize / invalid UTF-8); the
    /// read position is past it, so parsing can continue with the next one.
    Skipped,
}

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
    if !(1..=VERSION).contains(&version) {
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
            v => read_entry_v3(data, &mut pos, v >= 4),
        };
        match result {
            Some(ReadOutcome::Entry(e)) => entries.push(e),
            Some(ReadOutcome::Skipped) => {}
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
        let end = $pos.checked_add($n)?;
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

/// Read a length-prefixed text body.
/// `Some(Some(text))` = ok, `Some(None)` = rejected but skipped over
/// (oversize or invalid UTF-8), `None` = truncated file.
fn read_text_body(data: &[u8], pos: &mut usize) -> Option<Option<String>> {
    let content_len = try_read_u32!(data, pos) as usize;
    let content_bytes = try_read_bytes!(data, pos, content_len);
    if content_len > MAX_ENTRY_BYTES as usize {
        tracing::warn!("[persist] entry content too large ({content_len} bytes) — skipping");
        return Some(None);
    }
    match std::str::from_utf8(content_bytes) {
        Ok(s) => Some(Some(s.to_string())),
        Err(_) => {
            tracing::warn!("[persist] invalid UTF-8 in content — skipping entry");
            Some(None)
        }
    }
}

/// Read an optional length-prefixed string (label / color / tag).
fn read_opt_string(data: &[u8], pos: &mut usize, what: &str) -> Option<Option<String>> {
    if try_read_u8!(data, pos) != 1 {
        return Some(None);
    }
    let len = try_read_u32!(data, pos);
    if len > MAX_ENTRY_BYTES {
        tracing::warn!("[persist] {what} too large ({len} bytes)");
        return None;
    }
    let bytes = try_read_bytes!(data, pos, len as usize);
    match std::str::from_utf8(bytes) {
        Ok(s) => Some(Some(s.to_string())),
        Err(_) => {
            tracing::warn!("[persist] invalid UTF-8 in {what}");
            None
        }
    }
}

/// Verify the trailing CRC32 over `data[entry_start..*pos]`.
fn check_crc(data: &[u8], entry_start: usize, pos: &mut usize) -> Option<()> {
    let expected = crc32(&data[entry_start..*pos]);
    let stored   = try_read_u32!(data, pos);
    if stored != expected {
        tracing::warn!(
            "[persist] CRC32 mismatch (expected {expected:#010x}, got {stored:#010x})"
        );
        return None;
    }
    Some(())
}

/// Read a V1 entry (no color field). Sets color = None.
fn read_entry_v1(data: &[u8], pos: &mut usize) -> Option<ReadOutcome> {
    let entry_start = *pos;

    let id        = try_read_u64!(data, pos);
    let copied_at = try_read_u64!(data, pos);
    let pinned    = try_read_u8!(data, pos) != 0;
    let _         = try_read_bytes!(data, pos, 3); // pad

    let text  = read_text_body(data, pos)?;
    let label = read_opt_string(data, pos, "label")?;
    check_crc(data, entry_start, pos)?;

    let Some(text) = text else { return Some(ReadOutcome::Skipped) };
    Some(ReadOutcome::Entry(ClipboardEntry {
        id,
        content: ClipboardContent::Text(text),
        copied_at,
        pinned,
        label,
        color: None,
        tag: None,
    }))
}

/// Read a V2 entry (includes color field after label).
fn read_entry_v2(data: &[u8], pos: &mut usize) -> Option<ReadOutcome> {
    let entry_start = *pos;

    let id        = try_read_u64!(data, pos);
    let copied_at = try_read_u64!(data, pos);
    let pinned    = try_read_u8!(data, pos) != 0;
    let _         = try_read_bytes!(data, pos, 3); // pad

    let text  = read_text_body(data, pos)?;
    let label = read_opt_string(data, pos, "label")?;
    let color = read_opt_string(data, pos, "color")?;
    check_crc(data, entry_start, pos)?;

    let Some(text) = text else { return Some(ReadOutcome::Skipped) };
    Some(ReadOutcome::Entry(ClipboardEntry {
        id,
        content: ClipboardContent::Text(text),
        copied_at,
        pinned,
        label,
        color,
        tag: None,
    }))
}

/// Read a V3/V4 entry: reads type byte first, then dispatches text or image
/// layout. `with_tag` = V4 (tag field after the color).
fn read_entry_v3(data: &[u8], pos: &mut usize, with_tag: bool) -> Option<ReadOutcome> {
    let entry_type = try_read_u8!(data, pos);
    let entry_start = *pos;

    let id        = try_read_u64!(data, pos);
    let copied_at = try_read_u64!(data, pos);
    let pinned    = try_read_u8!(data, pos) != 0;
    let _         = try_read_bytes!(data, pos, 3); // pad

    let content = if entry_type == 0 {
        read_text_body(data, pos)?.map(ClipboardContent::Text)
    } else {
        // Image entry (type = 1)
        let hash_bytes = try_read_bytes!(data, pos, 32);
        let mut hash = [0u8; 32];
        hash.copy_from_slice(hash_bytes);
        let width  = try_read_u32!(data, pos);
        let height = try_read_u32!(data, pos);
        Some(ClipboardContent::Image { hash, width, height })
    };

    let label = read_opt_string(data, pos, "label")?;
    let color = read_opt_string(data, pos, "color")?;
    let tag   = if with_tag { read_opt_string(data, pos, "tag")? } else { None };
    check_crc(data, entry_start, pos)?;

    let Some(content) = content else { return Some(ReadOutcome::Skipped) };
    Some(ReadOutcome::Entry(ClipboardEntry { id, content, copied_at, pinned, label, color, tag }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cm-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("history.bin")
    }

    fn text(id: u64, s: &str) -> ClipboardEntry {
        ClipboardEntry::new_text(id, s.into())
    }

    /// Hand-built V2 file (no type byte, text only, label + color).
    fn v2_bytes(entries: &[(u64, &str, Option<&str>, Option<&str>)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        out.extend_from_slice(&[0u8; 6]);
        for (id, content, label, color) in entries {
            let mut buf = Vec::new();
            buf.extend_from_slice(&id.to_le_bytes());
            buf.extend_from_slice(&1000u64.to_le_bytes());
            buf.push(0);
            buf.extend_from_slice(&[0u8; 3]);
            buf.extend_from_slice(&(content.len() as u32).to_le_bytes());
            buf.extend_from_slice(content.as_bytes());
            write_label_color(&mut buf, &label.map(String::from), &color.map(String::from));
            let c = crc32(&buf);
            buf.extend_from_slice(&c.to_le_bytes());
            out.extend_from_slice(&buf);
        }
        out
    }

    #[test]
    fn roundtrip_text_image_and_meta() {
        let p = tmp("rt");
        let mut a = text(1, "hello");
        a.pinned = true;
        a.label = Some("L".into());
        a.color = Some("red".into());
        let b = ClipboardEntry::new_image(2, [7u8; 32], 10, 20);
        let e = PersistenceEngine::new(p);
        e.flush(&[&a, &b]).unwrap();
        let got = e.load();
        assert_eq!(got.len(), 2);
        assert!(got[0].pinned);
        assert_eq!(got[0].label.as_deref(), Some("L"));
        assert_eq!(got[0].color.as_deref(), Some("red"));
        assert!(matches!(got[1].content, ClipboardContent::Image { width: 10, height: 20, .. }));
    }

    #[test]
    fn v2_file_still_loads() {
        let p = tmp("v2");
        std::fs::write(&p, v2_bytes(&[(1, "a", Some("lbl"), None), (2, "b", None, Some("blue"))])).unwrap();
        let got = PersistenceEngine::new(p).load();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].label.as_deref(), Some("lbl"));
        assert_eq!(got[1].color.as_deref(), Some("blue"));
    }

    #[test]
    fn roundtrip_tag() {
        let p = tmp("tag");
        let mut a = text(1, "hello");
        a.tag = Some("Work".into());
        let mut b = ClipboardEntry::new_image(2, [3u8; 32], 5, 6);
        b.tag = Some("Ideas".into());
        let e = PersistenceEngine::new(p);
        e.flush(&[&a, &b, &text(3, "none")]).unwrap();
        let got = e.load();
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].tag.as_deref(), Some("Work"));
        assert_eq!(got[1].tag.as_deref(), Some("Ideas"));
        assert_eq!(got[2].tag, None);
    }

    #[test]
    fn v3_file_still_loads() {
        // V3: type byte, then id/copied_at/pinned/pad, body, label, color, crc.
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&3u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&[0u8; 6]);
        // text entry
        let mut buf = Vec::new();
        buf.extend_from_slice(&1u64.to_le_bytes());
        buf.extend_from_slice(&1000u64.to_le_bytes());
        buf.push(0);
        buf.extend_from_slice(&[0u8; 3]);
        buf.extend_from_slice(&2u32.to_le_bytes());
        buf.extend_from_slice(b"hi");
        write_label_color(&mut buf, &Some("L".into()), &Some("red".into()));
        let c = crc32(&buf);
        buf.extend_from_slice(&c.to_le_bytes());
        out.push(0);
        out.extend_from_slice(&buf);
        // image entry
        let mut buf = Vec::new();
        buf.extend_from_slice(&2u64.to_le_bytes());
        buf.extend_from_slice(&1001u64.to_le_bytes());
        buf.push(1);
        buf.extend_from_slice(&[0u8; 3]);
        buf.extend_from_slice(&[4u8; 32]);
        buf.extend_from_slice(&7u32.to_le_bytes());
        buf.extend_from_slice(&8u32.to_le_bytes());
        write_label_color(&mut buf, &None, &None);
        let c = crc32(&buf);
        buf.extend_from_slice(&c.to_le_bytes());
        out.push(1);
        out.extend_from_slice(&buf);

        let p = tmp("v3");
        std::fs::write(&p, out).unwrap();
        let got = PersistenceEngine::new(p).load();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].label.as_deref(), Some("L"));
        assert_eq!(got[0].tag, None);
        assert!(got[1].pinned);
        assert!(matches!(got[1].content, ClipboardContent::Image { width: 7, height: 8, .. }));
    }

    #[test]
    fn v1_file_still_loads() {
        // V1 = V2 layout without the color field.
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&[0u8; 6]);
        let mut buf = Vec::new();
        buf.extend_from_slice(&9u64.to_le_bytes());
        buf.extend_from_slice(&1000u64.to_le_bytes());
        buf.push(1);
        buf.extend_from_slice(&[0u8; 3]);
        buf.extend_from_slice(&3u32.to_le_bytes());
        buf.extend_from_slice(b"old");
        buf.push(0); // no label
        let c = crc32(&buf);
        buf.extend_from_slice(&c.to_le_bytes());
        out.extend_from_slice(&buf);
        let p = tmp("v1");
        std::fs::write(&p, out).unwrap();
        let got = PersistenceEngine::new(p).load();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, 9);
        assert!(got[0].pinned);
        assert!(matches!(&got[0].content, ClipboardContent::Text(t) if t == "old"));
    }

    #[test]
    fn oversize_entry_is_skipped_not_fatal() {
        let p = tmp("big");
        let big = text(1, &"x".repeat(MAX_ENTRY_BYTES as usize + 1));
        let small = text(2, "after");
        PersistenceEngine::new(p.clone()).flush(&[&big, &small]).unwrap();
        let got = PersistenceEngine::new(p).load();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, 2);
    }

    #[test]
    fn truncated_file_recovers_prefix() {
        let p = tmp("trunc");
        PersistenceEngine::new(p.clone()).flush(&[&text(1, "a"), &text(2, "b")]).unwrap();
        let data = std::fs::read(&p).unwrap();
        std::fs::write(&p, &data[..data.len() - 3]).unwrap();
        assert_eq!(PersistenceEngine::new(p).load().len(), 1);
    }

    #[test]
    fn history_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let p = tmp("perm");
        PersistenceEngine::new(p.clone()).flush(&[&text(1, "secret")]).unwrap();
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn load_tightens_existing_file_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let p = tmp("perm2");
        PersistenceEngine::new(p.clone()).flush(&[&text(1, "secret")]).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o664)).unwrap();
        PersistenceEngine::new(p.clone()).load();
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
