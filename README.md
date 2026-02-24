# Clipboard Manager

A lightweight clipboard history manager for Ubuntu/Linux with a GTK4 popup, global hotkey, and system tray icon. Copy anything — code, URLs, text — and retrieve any recent item instantly.

## Install dependencies

```bash
sudo apt-get install -y libgtk-4-dev libglib2.0-dev libdbus-1-dev
```

## Build

```bash
cargo build --release
```

## Install

```bash
sudo cp target/release/clipboard-manager /usr/local/bin/
```

## Run

```bash
clipboard-manager &
```

The app runs in the background, monitors your clipboard, and adds a tray icon.

## Usage

| Action | Result |
|---|---|
| **Ctrl+Alt+C** | Open clipboard history popup |
| Click an item | Copy it back to clipboard |
| **Escape** / click away | Close popup |
| Tray → Show History | Open popup |
| Tray → Quit | Exit the app |

## Config

Copy the default config and edit it:

```bash
mkdir -p ~/.config/clipboard-manager
cp config/default.toml ~/.config/clipboard-manager/config.toml
```

| Key | Default | Description |
|---|---|---|
| `hotkey` | `ctrl+alt+c` | Global shortcut to open popup |
| `max_history` | `50` | Number of items to remember |
| `deduplicate` | `true` | Skip duplicate entries |
| `popup_width` | `420` | Popup width in pixels |

## Autostart

On first run the app writes `~/.config/autostart/clipboard-manager.desktop` so it starts automatically on login.
