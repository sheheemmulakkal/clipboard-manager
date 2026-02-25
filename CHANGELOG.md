# Changelog

All notable changes to Clipboard Manager will be documented here.

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
