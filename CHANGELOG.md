# Changelog

All notable changes to Clipboard Manager will be documented here.

## [Unreleased]

### Added
- **Item labels and colors** — right-click any row to open a label editor.
  Set a short title and/or pick one of 8 Catppuccin Mocha accent colors.
  The title appears below the preview text; the color renders as a left
  border so important items stand out instantly.
- **Clipboard history persistence** — history (including pins, labels, and
  colors) is saved to `~/.local/share/clipboard-manager/history.bin` and
  restored on the next launch. Uses a compact binary format with CRC32
  checksums and partial-recovery on corruption.
- **Search / filter** — a search bar at the top of the popup filters items
  by content or label in real time. Pinned-first ordering is preserved.
  Esc clears the query; a second Esc closes the popup.
- **Paste to terminal** — ⌨ button sends Ctrl+Shift+V for terminal emulators.
- **Auto-daemonize** — the binary backgrounds itself on launch; no `&` needed.
- **`clipboard-manager reload`** subcommand — restarts the daemon to pick up
  config changes without logging out.
- Wayland support: paste via RemoteDesktop portal, hotkey via GlobalShortcuts
  portal (requires GNOME 43+ or compatible compositor).

### Fixed
- `apt remove` now also deletes `~/.local/share/clipboard-manager/` (history
  file), leaving no traces on uninstall.

## [1.0.0] - 2026-02-25
### Added
- Clipboard history monitoring (captures all copied text)
- Popup window triggered by Ctrl+Alt+C global hotkey
- Click any history item to paste it at the cursor
- Pin items to keep them at the top of the list
- Deduplication — duplicate copies don't fill the list
- Configurable via `~/.config/clipboard-manager/config.toml`
- System tray icon with quick access to popup and quit
- Autostart on login via XDG autostart
- Clear-with-undo support (configurable timeout)
- Popup follows cursor position (configurable)

### Requirements
- Ubuntu 20.04 / 22.04 / 24.04 on X11 (select "Ubuntu on Xorg" at login)

### Removed
- Wayland support removed for stability — X11 session required
