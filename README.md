# Clipboard Manager

A clipboard history popup for Ubuntu — press **Ctrl+Alt+C** to see everything
you've recently copied, and click any item to paste it instantly. Supports both
**text** and **screenshots / images**.

Inspired by the Windows Win+V experience, built natively for Ubuntu with
Rust and GTK4.

## Install

### One-line install (Ubuntu 22.04 / 24.04)
```bash
curl -fsSL https://raw.githubusercontent.com/sheheemmulakkal/clipboard-manager/master/install.sh | bash
```

Or download the `.deb` directly from the [Releases page](../../releases/latest).

### Requirements
- **Ubuntu 22.04 or newer** (amd64) — GTK4 (which this app is built on) is available by default only on Ubuntu 22.04+
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
| **Label / color** | Right-click the item row |
| **Clear all** | Click "Clear All" in the header (with undo) |
| **Search** | Type in the search bar at the top of the popup |
| **Keyboard navigation** | ↑ ↓ to move, Enter to paste, Esc to close |
| **Paste image** | Click an image row — the screenshot is restored to your clipboard and pasted |

> **Pinned items** are never evicted from history, even when the max history
> limit is reached. They appear at the top of the list with a colored left border.

> **Paste to terminal** uses Ctrl+Shift+V, which is the standard paste shortcut
> in most terminal emulators. Use this instead of a normal click when your
> target window is a terminal.

## Item labels and colors

Right-click any row to open the label editor:

- **Title** — type a short name for the item (e.g. "API key", "SSH command").
  The title appears below the preview text in every subsequent popup.
- **Color** — pick one of 8 Catppuccin Mocha accent colors, or "none".
  The chosen color appears as a left border on the row so important items
  stand out at a glance.

Click **Apply** (or press Enter in the title field) to save. Press Escape to
discard. Right-click the same row again to edit or clear the label.

Labels and colors are stored in the history file and survive restarts.

## Screenshots and images

When you copy an image to the clipboard (e.g. via PrtSc, Snipping Tool, or
any image editor), the clipboard manager captures it automatically:

- A **thumbnail** (240×135) is shown in the popup row instead of text.
- Clicking the row restores the full screenshot to your clipboard and pastes it.
- Images are **deduplicated by SHA-256** — copying the same screenshot twice
  adds only one entry.
- Full images and thumbnails are stored in
  `~/.local/share/clipboard-manager/images/` and are **never loaded into RAM**
  until you paste — only the file path and dimensions are kept in memory.
- Orphaned image files (whose history entry was evicted) are cleaned up
  automatically on the next app start.
- The terminal-paste button is hidden for image rows (terminals can't receive
  binary clipboard data via Ctrl+Shift+V).

> **Note:** image capture only triggers when the clipboard contains an image
> and no text. Entries copied from apps that put both image and text on the
> clipboard (e.g. LibreOffice cells) are captured as text.

## Search

The search bar is always visible at the top of the popup. Start typing to
filter items by content or label — the filter is case-insensitive and applied
on top of the pinned-first ordering. Image entries match on `image W×H` or
their label. Press Esc once to clear the search, and again to close the popup.

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

### Applying config changes

Config is read once at startup. To apply any changes, restart the app — no logout needed:

```bash
clipboard-manager reload
```

The `reload` command stops the running daemon and starts a fresh one in the background automatically.

## Data files

| File | Purpose |
|---|---|
| `~/.config/clipboard-manager/config.toml` | User configuration |
| `~/.local/share/clipboard-manager/history.bin` | Clipboard history (text, labels, colors, pins) |
| `~/.local/share/clipboard-manager/images/` | Captured screenshots (full PNGs + 240×135 thumbnails) |

## Uninstall
```bash
sudo apt remove clipboard-manager
```
This removes the binary, stops the running process, clears autostart entries, and deletes both `~/.config/clipboard-manager/` and `~/.local/share/clipboard-manager/`.

## Build from source
```bash
# Install dependencies
sudo apt install libgtk-4-dev libglib2.0-dev libx11-dev libxtst-dev libgdk-pixbuf-2.0-dev pkg-config build-essential

# Build
cargo build --release

# Build installable .deb
cargo install cargo-deb
cargo deb
sudo apt install ./target/debian/clipboard-manager_*.deb
```

## Testing locally

Three levels depending on what you're changing:

**1. App only (fastest) — no install needed:**
```bash
cargo build --release && ./target/release/clipboard-manager
```
The app backgrounds itself automatically. To see log output instead, set `RUST_LOG`:
```bash
RUST_LOG=debug ./target/release/clipboard-manager
```
Kill it with `pkill -f clipboard-manager`.

**2. Full .deb lifecycle — tests install/remove scripts end-to-end:**
```bash
cargo build --release
cargo deb
# Copy to /tmp so apt can access it as the _apt user (home dirs are not world-readable)
cp target/debian/clipboard-manager_*.deb /tmp/
sudo apt install /tmp/clipboard-manager_*.deb
# test the app...
sudo apt remove clipboard-manager
```

**3. Package scripts only — no rebuild needed:**

When only `prerm`/`postrm` changed, swap them in-place and remove:
```bash
sudo cp packaging/debian/prerm  /var/lib/dpkg/info/clipboard-manager.prerm
sudo cp packaging/debian/postrm /var/lib/dpkg/info/clipboard-manager.postrm
sudo apt remove clipboard-manager
```

Or run scripts directly to check for errors:
```bash
sudo bash packaging/debian/prerm remove
sudo bash packaging/debian/postrm remove
```

**Verify clean removal:**
```bash
ls ~/.config/clipboard-manager/            2>/dev/null || echo "clean"
ls ~/.local/share/clipboard-manager/       2>/dev/null || echo "clean"
ls ~/.config/autostart/ | grep clipboard               || echo "clean"
pgrep -f clipboard-manager                             || echo "clean"
```

## License
MIT
