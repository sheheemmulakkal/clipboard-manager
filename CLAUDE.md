# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Install system dependencies (Ubuntu)
sudo apt install libgtk-4-dev libglib2.0-dev libx11-dev libxtst-dev pkg-config build-essential

# Build debug
cargo build

# Build release (size-optimized: opt-level=z, lto, strip)
cargo build --release

# Run
cargo run
# or with logging
RUST_LOG=debug cargo run

# Run on X11 explicitly (useful when $WAYLAND_DISPLAY is set)
GDK_BACKEND=x11 cargo run

# Check/lint
cargo check
cargo clippy

# Build installable .deb package
cargo install cargo-deb
cargo deb
sudo apt install ./target/debian/clipboard-manager_*.deb
```

There are no automated tests in this project.

## Features / Feature Flags

- `ui` (default) — enables GTK4 (`gtk4`, `glib`, `gdk4`, `gdk4-x11`)
- `persist` — optional SQLite persistence via `rusqlite` (not wired up yet)

## Architecture

This is a GTK4 desktop app running a single-process event loop. All UI and async clipboard work runs on the GTK/glib main thread. Background threads are used only for hotkey listening and paste dispatch.

### Dual-backend Strategy Pattern

The codebase uses the Strategy pattern twice — once for hotkeys and once for platform operations — with runtime detection via `$WAYLAND_DISPLAY`.

**Platform** (`src/platform/`):
- `Platform` trait: `capture_active_window`, `paste`, `cursor_position`, `move_popup`, `button1_held`
- `X11Platform` — uses `x11rb` for everything: XTest key injection for paste, `query_pointer` for cursor, `configure_window` for popup positioning via `gdk4-x11`
- `WaylandPlatform` — uses `ashpd` RemoteDesktop portal for paste; cursor/move are no-ops
- `platform::detect()` returns `Arc<dyn Platform>` — the `Arc` is needed because it's passed into both GTK closures and background `std::thread::spawn` threads

**Hotkey** (`src/hotkey/`):
- `HotkeyManager` trait: `start(Box<dyn Fn() + Send + Sync>)` / `stop()`
- `X11HotkeyManager` — XGrabKey via x11rb; spins a background thread
- `WaylandHotkeyManager` — `org.freedesktop.portal.GlobalShortcuts` via ashpd/tokio; gracefully degrades with a warning if the compositor doesn't support it
- `hotkey::detect(hotkey_str)` returns `Box<dyn HotkeyManager>`

### Application Lifecycle (`src/app.rs`)

`App::run()` wires everything together inside `app.connect_activate`:

1. Calls `platform::detect()` and `hotkey::detect()`
2. Starts `ClipboardMonitor` (polls GDK clipboard every 500ms on the main thread)
3. Creates `std::sync::mpsc::sync_channel` pairs: `hotkey_rx`, `tray_show_rx`, `tray_quit_rx`
4. Starts `ksni` system tray in a background thread
5. Runs a **50ms glib poll loop** that drains the channels and triggers popup show or app quit

When the popup is triggered, a **repopulate closure** is built inside `Rc<RefCell<Option<Box<dyn Fn()>>>>` and immediately called. This pattern avoids circular closure references: `on_remove` and `on_pin` callbacks mutate the store, then call `repop.borrow().as_ref().unwrap()()` to refresh the list.

**Search state** lives in two `Rc<RefCell<...>>` values created once alongside the popup (outside the poll loop): `search_query` (current filter string) and `repop_shared` (pointer to the current session's repopulate closure, updated on each popup open). `popup.connect_search_changed` writes to `search_query` and calls `repop_shared`. Inside the repopulate closure, `filter_entries(sorted_entries(...), &query)` applies the filter before passing entries to `popup.populate()`.

### Paste Flow

1. Hotkey fires → background thread calls `platform.capture_active_window()` → sends `Option<u64>` (X11 window ID) over `hotkey_tx`
2. Poll loop receives it → triggers popup populate + show
3. User selects an item → `on_select` closure: sets GDK clipboard text, hides popup, then after 200ms delay spawns a thread calling `platform.paste(prev_window_id)`
4. X11 paste: `XActivateWindow` on the stored ID, then XTest Ctrl+V keypress
5. Wayland paste: ashpd `RemoteDesktop` portal `notify_keyboard_keycode` for Ctrl+V

### Store (`src/store/`)

`Store` trait is `Box<dyn Store>` wrapped in `Rc<RefCell<...>>` for single-threaded interior mutability. `MemoryStore` is the only implementation; it caps history at `max_history` by evicting the oldest non-pinned entry. Pinned entries are never evicted.

### UI (`src/ui/`)

- `ClipboardPopup` owns the GTK `Window`. All callbacks (`on_select`, `on_copy`, `on_remove`, `on_pin`, `on_clear`) are stored as `Rc<RefCell<Option<Rc<dyn Fn(...)>>>>` so they can be replaced on each repopulate without rebuilding the window.
- `item_row.rs` — `build_item_row()` returns a GTK widget + `RowAction` enum; pin/delete buttons are CSS-opacity-hidden until hover.
- Popup positioning: `show_at_cursor()` reads `platform.cursor_position()`, shows the window, then after 50ms defers `platform.move_popup()`. Falls back to `show_centered()` on Wayland.
- CSS (Catppuccin Mocha palette) is embedded via `include_str!("../../assets/style.css")` at compile time.
- **Search**: `SearchEntry` is placed between the header and the list. On each popup open the query is cleared and the entry is focused. Typing filters entries via `filter_entries()` in `app.rs` (case-insensitive substring match on content and label). Pinned-first ordering is preserved because `sorted_entries()` runs before filtering. Down arrow from the search entry jumps to list row 0; Escape clears search text first, then closes on a second press.

### Config (`src/config.rs`)

Loaded from `~/.config/clipboard-manager/config.toml` (TOML, serde). Falls back to defaults if the file doesn't exist. Key fields: `max_history`, `hotkey`, `popup_follow_cursor`, `clear_undo_timeout_secs`, `popup_width`, `popup_max_items`, `show_timestamps`, `deduplicate`.

### Concurrency Model

- GTK main thread: all UI, clipboard polling (GDK async), 50ms glib timeout loop
- Hotkey thread (background): one per `HotkeyManager::start()` call
- Paste thread (transient): `std::thread::spawn` inside the 200ms glib timeout callback — required because `platform.paste()` may block
- Cross-thread communication: only via `std::sync::mpsc::sync_channel` (capacity 1), drained in the glib poll loop
- `Arc<dyn Platform>` is shared between main thread and background threads; `Rc<RefCell<...>>` is used everywhere else (single-threaded GTK side only)

## Release Process

Releases are triggered by pushing a `v*` tag. The GitHub Actions workflow (`.github/workflows/release.yml`) builds on ubuntu-22.04, runs `cargo deb`, and uploads the `.deb` to a GitHub Release.
