# Architecture

Clipboard Manager is a single-process GTK4 daemon. Everything that touches
GTK or the history store runs on the glib main thread; background threads
only listen for hotkeys, serve the tray icon and send paste keystrokes.

## Overview

```
 hotkey thread ─┐                           ┌─ ClipboardMonitor (GDK clipboard `changed`)
 tray thread ───┼─ AppEvent ─▶ async_channel│            │ add / bump
 CLI (D-Bus) ───┘   (Show, Toggle, …)       │            ▼
                         │                  │      Store (MemoryStore + PersistentStore)
                         ▼                  │            ▲
                    Controller ◀────────────┘            │ touch / meta / remove …
                     │      ▲                            │
            populate │      │ PopupEvent (Row, Search, Chip, Menu, ClearAll)
                     ▼      │
                 ClipboardPopup (window, header, search, chips, rows, popovers)
                     │
                 Platform (X11Platform | WaylandPlatform): paste, window focus, placement
```

## Directory layout

```
src/
  main.rs            CLI dispatch, daemonize, Wayland → XWayland backend choice
  app.rs             GTK application: command-line handling, wiring, startup GC
  controller.rs      Controller: store + popup + platform; handles all events
  events.rs          AppEvent (threads → main), PopupEvent / RowAction / MenuAction
  cli.rs             Command parsing and `list` output (pure)
  config.rs          AppConfig (TOML), theme name, colour/size overrides
  paths.rs           Data/config/state dirs (0700), image paths, dev profile ids
  notify.rs          Desktop notifications for errors the user must act on
  tray.rs            StatusNotifierItem tray icon (ksni)
  clipboard/
    entry.rs         ClipboardEntry, ClipboardContent, EntryMeta
    kind.rs          ContentKind detection: URL, email, path, shell, code, colour, secret
    monitor.rs       Capture: size caps, password hint, ignored apps, pause, images
  store/
    mod.rs           Store trait
    memory.rs        MemoryStore: ordering, dedup-as-bump, eviction, restore, expiry
    persistent.rs    PersistentStore: MemoryStore + flush on every mutation
    engine.rs        history.bin reader/writer (V1–V4)
  hotkey/            X11 XGrabKey · Wayland portal → GNOME shortcut → evdev
  platform/          X11Platform (x11rb, XTest) · WaylandPlatform (RemoteDesktop portal)
  ui/
    popup.rs         Window, header, search, chips, list, undo bar, keyboard, focus
    item_row.rs      One row: dot, kind tile/thumbnail, title, tag pill, actions, pin
    context_menu.rs  Right-click menu with tag and colour sub-pages
    editor.rs        Edit title/content popover
    preview.rs       Full preview popover
    filter.rs        Ordering, search and chip filtering (pure)
    format.rs        Relative time, titles, subtitles, preview header (pure)
    icons.rs         Embedded SVG icons → HiDPI textures
    theme.rs         Theme tokens, colour palette, default tags
    style.rs         CSS generated from the theme
assets/icons/        24×24 stroke SVGs (currentColor)
```

## Key design points

### Controller + events
The popup never touches the store. Every user action becomes a
`PopupEvent` handed to the `Controller`, which mutates the store and calls
`refresh()`: filter → sort → `popup.populate()`. Background threads send
`AppEvent`s through an `async_channel` that a `glib::spawn_future_local`
loop feeds to the controller.

Popovers (menu, editor, preview) emit their action **after** they have
closed and been unparented: the action usually rebuilds the list, and
the row that owned the popover would otherwise be destroyed while it still
had a child popover.

### Focus loss closes the popup
The popup hides when it loses focus, except while a popover is open (a
counter `suppress_close` maintained by `track_popover`), while "keep open"
is on, or during a header drag (see the drag-tracking comment in popup.rs).

### Placement
On X11 the popup opens next to the cursor, clamped to the work area of
the monitor under the cursor. Mutter places a newly mapped window itself,
so the move is (re)applied after the first paint and on activation.

### Clipboard capture
`ClipboardMonitor` reacts to GDK's clipboard `changed` signal (XFixes on
X11). It ignores our own clipboard changes (the controller records those
as "touch"), paused capture, content carrying the
`x-kde-passwordManagerHint` target (queried from X11 directly, because GDK
hides non-MIME targets), and copies made while an ignored app is focused.

### Re-copy = move to top
With `deduplicate = true`, adding content that is already stored moves the
existing entry to the newest position with a fresh timestamp and keeps its
id, pin, label, colour and tag.

### Wayland
On a Wayland session with XWayland available, `main` sets
`GDK_BACKEND=x11`: an XWayland client can read the clipboard in the
background (native Wayland clients only while focused). The platform and
hotkey backends still key on the *session*: paste goes through the
RemoteDesktop portal (restore token saved, session closed after 10 s idle so
GNOME's remote-control indicator doesn't stay on), and the hotkey falls back
from the GlobalShortcuts portal to a GNOME custom shortcut running
`clipboard-manager toggle`, then to evdev.

### Single instance and CLI
GApplication with `HANDLES_COMMAND_LINE`. A second `clipboard-manager
<command>` forwards its command line over D-Bus to the running instance;
results come back as the exit status (printing into the caller's terminal
needs glib 2.80). `list` reads history.bin in the client process.

## Data model

```rust
enum ClipboardContent { Text(String), Image { hash: [u8; 32], width: u32, height: u32 } }
struct ClipboardEntry { id, content, copied_at, pinned, label, color, tag, note }
```

Images are stored as `images/<sha256>.png` plus `<sha256>_thumb.png`;
orphans are deleted at startup.

## Persistence — history.bin (V5)

```
header: magic "CLIPMGR1" | version u16 | flags u16 | count u32 | reserved[6]
entry:  type u8 (0 text, 1 image)
        id u64 | copied_at u64 | pinned u8 | pad[3]
        text:  len u32 | utf8   —or—   image: hash[32] | width u32 | height u32
        label: has u8 [len u32 | utf8]
        color: has u8 [len u32 | utf8]
        tag:   has u8 [len u32 | utf8]            (V4)
        note:  has u8 [len u32 | utf8]            (V5)
        crc32 u32 over everything after the type byte
```

The file is written to `history.bin.tmp` (mode 0600, fsync) and renamed.
The reader accepts V1–V5; an oversize or invalid-UTF-8 entry is skipped
(its length is known), and a CRC mismatch or truncation stops reading and
keeps what was read so far.

## Testing

- `cargo test` covers the pure logic: store, persistence (all versions,
  corruption, permissions), kind detection, filtering, formatting, CLI
  parsing, theme, placement maths.
- `scripts/dev-run.sh` runs a build in a nested X server with its own
  D-Bus session and data directories, for manual testing without touching
  your real clipboard or installed instance.
- `CLIPBOARD_MANAGER_PROFILE=dev` runs a development instance next to an
  installed one.

## Known limitations

- Native Wayland without XWayland: history only records while the popup is
  focused (no data-control protocol support yet).
- On Wayland the popup opens centred (no global cursor position).
- The ignored-apps check needs X11 window information (not available for
  native Wayland apps); the password-manager hint works everywhere.
