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

- No automated tests — logic is verified manually.
- `ui` feature (default) pulls in GTK4, sha2, gdk-pixbuf. `persist` feature (default) is pure std.
- All UI and clipboard work runs on the GTK/glib main thread. Background threads only for hotkey listening and paste dispatch.
- `Rc<RefCell<...>>` throughout the GTK side; `Arc<...>` only where threads share state.
- The repopulate pattern: every mutating callback (remove/pin/label) calls the stored repopulate closure instead of directly touching the list.
- V3 binary format for history.bin — type-prefixed entries (0 = text, 1 = image). V1/V2 files load transparently.
