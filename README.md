# Clipboard Manager

A clipboard history popup for Ubuntu — press **Super+V** to see everything
you've recently copied, and click any item to paste it instantly.

Inspired by the Windows Win+V experience, built natively for Ubuntu with
Rust and GTK4.

## Install

### One-line install (Ubuntu 20.04 / 22.04 / 24.04)
```bash
curl -fsSL https://raw.githubusercontent.com/sheheemmulakkal/clipboard-manager/main/install.sh | bash
```

Or download the `.deb` directly from the [Releases page](../../releases/latest).

### Requirements
- Ubuntu 20.04, 22.04, or 24.04 (amd64)
- X11 session — at login, choose **"Ubuntu on Xorg"**
- `xdotool` (installed automatically)

## Usage
1. The app starts automatically on login (no action needed)
2. Copy text anywhere as usual
3. Press **Super+V** — a popup shows your clipboard history
4. Click any item — it pastes at your cursor

To start it immediately after install (without logging out):
```bash
GDK_BACKEND=x11 clipboard-manager &
```

## Configuration
Edit `~/.config/clipboard-manager/config.toml`:
```toml
max_history = 50
hotkey = "super+v"
paste_delay_ms = 150
```

## Uninstall
```bash
sudo apt remove clipboard-manager
```
Your config and history are kept at `~/.config/clipboard-manager/`.
Delete that folder to remove everything.

## Build from source
```bash
# Install dependencies
sudo apt install libgtk-4-dev libx11-dev libxtst-dev xdotool pkg-config

# Build
cargo build --release

# Build installable .deb
cargo install cargo-deb
cargo deb
sudo apt install ./target/debian/clipboard-manager_*.deb
```

## License
MIT
