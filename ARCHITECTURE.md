# Architecture

A deep-dive into how Clipboard Manager is structured. Read this before
making a non-trivial contribution.

---

## Overview

Clipboard Manager is a single-binary GTK4 desktop app for Ubuntu. It runs as
a background daemon and surfaces a popup window on a global hotkey. All UI
work runs on the GTK/glib main thread; background threads are used only for
hotkey listening and paste dispatch.

```
┌─────────────────────────────────────────────────────┐
│                   GTK main thread                   │
│                                                     │
│  ClipboardMonitor ──► Store ──► popup.populate()   │
│  (500ms poll)         │                             │
│                       │        ClipboardPopup       │
│  glib poll loop ◄─────┘        (GTK Window)        │
│  (50ms)                                             │
└──────────┬──────────────────────────┬───────────────┘
           │ mpsc channel             │ mpsc channel
    ┌──────▼──────┐            ┌──────▼──────┐
    │   Hotkey    │            │  ksni tray  │
    │   thread    │            │   thread    │
    └─────────────┘            └─────────────┘
```

---

## Directory layout

```
src/
├── main.rs                 Entry point — daemon / CLI dispatch
├── app.rs                  App struct, GTK activation, all closure wiring
├── config.rs               AppConfig — loaded from config.toml via serde
│
├── clipboard/
│   ├── entry.rs            ClipboardContent enum + ClipboardEntry struct
│   └── monitor.rs          ClipboardMonitor — 500ms GDK clipboard poll
│
├── store/
│   ├── mod.rs              Store trait
│   ├── memory.rs           MemoryStore — in-memory VecDeque implementation
│   ├── engine.rs           PersistenceEngine — V3 binary format read/write
│   └── persistent.rs       PersistentStore — decorator that flushes on every write
│
├── platform/
│   ├── mod.rs              Platform trait + detect() factory
│   ├── x11.rs              X11Platform — x11rb: paste, cursor, window move
│   └── wayland.rs          WaylandPlatform — ashpd portals: paste
│
├── hotkey/
│   ├── mod.rs              HotkeyManager trait + detect() factory
│   ├── x11.rs              X11HotkeyManager — XGrabKey via x11rb
│   ├── wayland.rs          WaylandHotkeyManager — GlobalShortcuts portal
│   └── evdev.rs            evdev fallback — reads /dev/input/event* directly
│
└── ui/
    ├── mod.rs
    ├── popup.rs            ClipboardPopup — the GTK Window + all callbacks
    ├── item_row.rs         build_item_row() — one list row per entry
    └── style.rs            generate_css() — runtime CSS generation
```

---

## Key design patterns

### 1. Strategy pattern — Platform and Hotkey

Both `Platform` and `HotkeyManager` use the Strategy pattern. The concrete
implementation is chosen once at startup by inspecting `$WAYLAND_DISPLAY`:

```rust
// platform::detect() returns Arc<dyn Platform>
// hotkey::detect()   returns Box<dyn HotkeyManager>
```

`Arc<dyn Platform>` is used (not `Box`) because the platform is shared between
the GTK main thread and background `std::thread::spawn` threads.

### 2. Repopulate pattern

The popup list is rebuilt from scratch on every change (pin, remove, search).
A repopulate closure is stored in `Rc<RefCell<Option<Box<dyn Fn()>>>>` and
called by every mutating callback:

```
on_remove/on_pin/on_label
  └── store.borrow_mut().mutate(...)
  └── repop.borrow().as_ref().unwrap()()   ← rebuilds the list
```

This avoids circular closure references: no callback holds a direct reference
to another callback.

### 3. Callback storage in ClipboardPopup

`populate()` is called on every repopulate. To avoid rebuilding the GTK
Window, callbacks are stored as `Rc<RefCell<Option<Rc<dyn Fn(...)>>>>` and
replaced in-place:

```rust
*self.on_select.borrow_mut() = Some(Rc::new(new_closure));
```

### 4. Cross-thread communication

Only `std::sync::mpsc::sync_channel` (capacity 1) is used to cross thread
boundaries. The glib 50ms poll loop drains these channels on the main thread:

```
hotkey thread  ──► hotkey_tx  ──► [poll loop] ──► show popup
tray thread    ──► tray_tx    ──► [poll loop] ──► show popup / quit
```

`Rc<RefCell<...>>` is used everywhere on the GTK side (single-threaded).
`Arc<Mutex<...>>` is only used for the `show_tx` sender shared with the
GTK re-activation handler.

---

## Data model

### ClipboardContent

```rust
pub enum ClipboardContent {
    Text(String),
    Image { hash: [u8; 32], width: u32, height: u32 },
}
```

`ClipboardEntry.content` is this enum. Image files live on disk — only the
metadata is in RAM.

### Image file layout

```
~/.local/share/clipboard-manager/images/
  {sha256_hex}.png           ← full resolution PNG
  {sha256_hex}_thumb.png     ← 240×135 thumbnail (pre-generated at capture)
```

Images are deduplicated by SHA-256. A startup GC (`gc_image_files` in
`app.rs`) deletes files whose store entry has been evicted.

### Store trait

```rust
pub trait Store: Send + Sync {
    fn add(&mut self, entry: ClipboardEntry);
    fn remove(&mut self, id: u64);
    fn get_all(&self) -> Vec<&ClipboardEntry>;
    fn contains_text(&self, text: &str) -> bool;
    fn contains_image_hash(&self, hash: &[u8; 32]) -> bool;
    fn set_pinned(&mut self, id: u64, pinned: bool);
    fn set_label(&mut self, id: u64, label: Option<String>, color: Option<String>);
    fn clear_unpinned(&mut self);
    fn clear(&mut self);
    fn len(&self) -> usize;
}
```

`PersistentStore` wraps `MemoryStore` as a decorator: every mutating method
delegates to the inner store, then calls `flush()`. All eviction and dedup
logic lives in `MemoryStore`.

---

## Persistence — binary format (V3)

File: `~/.local/share/clipboard-manager/history.bin`

**Header** (22 bytes):
```
CLIPMGR1 (8)  |  version u16 LE (2)  |  flags u16 (2)  |  count u32 (4)  |  reserved (6)
```

**Text entry** (type byte = 0):
```
type(1)=0 | id(8) | copied_at(8) | pinned(1) | pad(3) | content_len(4) | content(n)
| has_label(1) | [label_len(4) | label(n)]
| has_color(1) | [color_len(4) | color(n)]
| crc32(4)    ← CRC covers from id(8) onward
```

**Image entry** (type byte = 1):
```
type(1)=1 | id(8) | copied_at(8) | pinned(1) | pad(3) | hash(32) | width(4) | height(4)
| has_label(1) | [label_len(4) | label(n)]
| has_color(1) | [color_len(4) | color(n)]
| crc32(4)
```

V1 and V2 files are read transparently (as all-text) and re-saved as V3 on
the next flush. CRC32 mismatches are skipped with a warning; partial files
are recovered up to the corrupt entry.

---

## Clipboard capture flow

```
glib 500ms timer
  └── clipboard.formats()           check MIME types (cheap, no data transfer)
        ├── image/* only  ──► read_texture_async()
        │                         save PNG → hash → dedup → thumbnail → store.add()
        └── text present  ──► read_text_async()
                                  dedup → store.add()
```

**Note:** image capture runs synchronously on the GTK main thread inside the
async callback. For a 1080p screenshot this is ~200–500ms of blocking work
(PNG encode + hash + thumbnail). This is a known limitation — see
[#TODO: issue link] for the tracked improvement.

---

## Paste flow

```
1. Hotkey fires (background thread)
     └── platform.capture_active_window()   ← X11: _NET_ACTIVE_WINDOW
     └── hotkey_tx.send(prev_window_id)

2. Poll loop (main thread, 50ms)
     └── popup.populate(...) + show

3. User clicks row
     └── set_clipboard_content(content, image_dir)
           Text  → clipboard.set_text()
           Image → Texture::from_file() → clipboard.set_texture()
     └── popup.hide()
     └── glib::timeout (200ms)
           └── std::thread::spawn
                 └── platform.paste(prev_window_id)
                       X11      → XActivateWindow + XTest Ctrl+V
                       Wayland  → RemoteDesktop portal Ctrl+V
```

---

## CSS generation

CSS is generated at runtime by `src/ui/style.rs` — not loaded from a static
file. This lets user color/size config flow directly into the CSS without
string interpolation hacks. The `generate_css(colors, sizes)` function takes
resolved color values and emits a single CSS string loaded via `CssProvider`.

---

## Feature flags

| Flag | Default | What it enables |
|---|---|---|
| `ui` | ✅ | GTK4, gdk4, gdk4-x11, sha2, gdk-pixbuf |
| `persist` | ✅ | Binary history persistence (pure std, no external deps) |

Build without UI (for testing store/config logic):
```bash
cargo build --no-default-features --features persist
```

---

## Adding a new feature — checklist

- **New clipboard content type**: add a variant to `ClipboardContent`, handle
  it in `engine.rs` (new type byte), `monitor.rs` (capture), `item_row.rs`
  (display), and `app.rs` (`set_clipboard_content` + `filter_entries`).

- **New store operation**: add to the `Store` trait in `mod.rs`, implement in
  `MemoryStore`, delegate in `PersistentStore`.

- **New UI callback**: add the `Rc<RefCell<Option<...>>>` field to
  `ClipboardPopup`, wire it in `populate()`, and add the matching argument.

- **New platform operation**: add to the `Platform` trait, implement in both
  `X11Platform` and `WaylandPlatform` (even if the Wayland version is a no-op).

---

## Known limitations

- Image capture blocks the GTK main thread (~200–500ms per screenshot).
- Wayland hotkey requires a compositor that supports `GlobalShortcuts` portal
  (GNOME 43+, KDE Plasma 6). Falls back to evdev on failure.
- No automated test suite — logic is tested manually.
