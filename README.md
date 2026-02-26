# Clipboard Manager

A clipboard history popup for Ubuntu — press **Ctrl+Alt+C** to see everything
you've recently copied, and click any item to paste it instantly.

Inspired by the Windows Win+V experience, built natively for Ubuntu with
Rust and GTK4.

## Install

### One-line install (Ubuntu 20.04 / 22.04 / 24.04)
```bash
curl -fsSL https://raw.githubusercontent.com/sheheemmulakkal/clipboard-manager/master/install.sh | bash
```

Or download the `.deb` directly from the [Releases page](../../releases/latest).

### Requirements
- Ubuntu 20.04, 22.04, or 24.04 (amd64)
- X11 or Wayland session
  - X11: full support (paste, cursor-following popup, hotkey)
  - Wayland: paste via RemoteDesktop portal, hotkey via GlobalShortcuts portal (requires a compatible compositor e.g. GNOME 43+)

## Usage

1. The app starts automatically after install — no reboot or logout needed
2. Copy text anywhere as usual
3. Press **Ctrl+Alt+C** — a popup shows your clipboard history
4. Interact with any item:

| Action | How |
|---|---|
| **Paste** | Click the item row |
| **Paste to terminal** | Click the ⌨ button (sends Ctrl+Shift+V) |
| **Copy only** (no paste) | Click the ⎘ button |
| **Pin** (keep forever) | Click the ○ / ● button |
| **Delete** | Click the ✕ button |
| **Clear all** | Click "Clear All" in the header (with undo) |
| **Keyboard navigation** | ↑ ↓ to move, Enter to paste, Esc to close |

> **Pinned items** are never evicted from history, even when the max history
> limit is reached. They appear at the top of the list with a colored left border.

> **Paste to terminal** uses Ctrl+Shift+V, which is the standard paste shortcut
> in most terminal emulators. Use this instead of a normal click when your
> target window is a terminal.

## Configuration

Edit `~/.config/clipboard-manager/config.toml` (created with defaults on first run):

```toml
max_history              = 50
hotkey                   = "ctrl+alt+c"
popup_follow_cursor      = true
clear_undo_timeout_secs  = 5
deduplicate              = true
nerd_font                = false   # set true if a Nerd Font is installed
```

### Nerd Font icons

When `nerd_font = true` the action buttons use Nerd Font (Material Design) icons:

| Button | Unicode (default) | Nerd Font |
|---|---|---|
| Copy | ⎘ | 󰆏 |
| Paste to terminal | ⌨ | 󰆍 |
| Pin (off) | ○ | 󰐃 |
| Pin (on) | ● | 󰐄 |
| Delete | ✕ | 󰗨 |

### Custom colors

Add a `[colors]` section. All fields are optional — any unset field falls back
to the active GTK4 system theme. Setting a base color auto-derives related
slots (e.g. `text` → `text_muted`, `row_hover`; `accent` → `selection`).

```toml
[colors]
background        = "#1e1e2e"
header_background = "#181825"   # default: shade(background, 0.92)
border            = "#45475a"
text              = "#cdd6f4"
text_muted        = "#6c7086"   # default: alpha(text, 0.5)
accent            = "#89b4fa"   # pin highlight + selection tint
error             = "#f38ba8"   # delete / clear hover color
row_hover         = "#313244"   # default: alpha(text, 0.06)
selection         = "#45475a"   # default: alpha(accent, 0.25)
```

### Custom sizes

Add a `[sizes]` section. All values are in CSS `px` units.

```toml
[sizes]
font_preview = 13   # clipboard item text
font_time    = 11   # timestamp label
font_title   = 13   # popup header title
font_buttons = 13   # action button icons
font_undo    = 12   # undo bar text
row_height   = 44   # minimum row height
```

## Uninstall
```bash
sudo apt remove clipboard-manager
```
Your config is kept at `~/.config/clipboard-manager/`.
Delete that folder to remove everything.

## Build from source
```bash
# Install dependencies
sudo apt install libgtk-4-dev libglib2.0-dev libx11-dev libxtst-dev pkg-config build-essential

# Build
cargo build --release

# Build installable .deb
cargo install cargo-deb
cargo deb
sudo apt install ./target/debian/clipboard-manager_*.deb
```

## License
MIT
