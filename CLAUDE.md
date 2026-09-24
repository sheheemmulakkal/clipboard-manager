# CLAUDE.md

This file provides guidance to Claude Code when working in this repository.
For a full architectural overview see [ARCHITECTURE.md](ARCHITECTURE.md).

## Build commands

```bash
# Install system dependencies (Ubuntu)
sudo apt install libgtk-4-dev libglib2.0-dev libx11-dev libxtst-dev libgdk-pixbuf-2.0-dev pkg-config build-essential

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

# Build installable .deb
cargo install cargo-deb
cargo deb
sudo apt install ./target/debian/clipboard-manager_*.deb
```

## Key facts

- `cargo test` covers the pure logic (store, persistence, kind detection,
  filtering, formatting, CLI, theme). GTK behaviour is verified manually —
  `scripts/dev-run.sh` runs an isolated instance in a nested X server.
- `cargo clippy --all-targets -- -D warnings` must stay clean (CI enforces it).
- `ui` feature (default) pulls in GTK4, sha2, gdk-pixbuf. `persist` feature (default) is pure std.
- Must build against GTK 4.6 / glib 2.72 (Ubuntu 22.04): avoid APIs gated
  behind newer gtk4-rs/gio feature flags.
- All UI and store work runs on the GTK/glib main thread. Background threads
  (hotkey, tray, paste) send `AppEvent`s over an `async_channel`.
- The popup never mutates the store: it emits `PopupEvent`s; `Controller`
  mutates the store and calls `refresh()` to rebuild the list.
- Popover actions are emitted after the popover closes and is unparented
  (they usually rebuild the row that owns the popover).
- History format is V4 (tags added). V1–V3 files load transparently.
- On Wayland the UI runs on XWayland (`GDK_BACKEND=x11`), paste uses the
  RemoteDesktop portal.
