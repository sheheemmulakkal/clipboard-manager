# Clipboard Manager — Roadmap / TODO

Status as of 2026-09-24 (v1.2.0). Work roughly top to bottom: each phase
builds on the one before it.

Size: **S** = under an hour · **M** = a few hours · **L** = a day or more

---

## Phase 0 — Critical bug fixes

- [ ] **S** Cap text size at capture time (e.g. 1 MB, configurable). Today there is
      no limit on capture, but the loader rejects entries over 10 MB and then stops
      reading the file, so every entry after it is lost (`store/engine.rs:183-188`,
      `clipboard/monitor.rs:194`).
- [ ] **S** Make the loader skip an oversized entry instead of stopping at it (the
      length is known, so it can seek past it).
- [ ] **M** Stop the image churn: every 500 ms the monitor re-encodes, writes,
      reads and hashes any image sitting on the clipboard (`monitor.rs:100-145`).
      Hash the texture bytes in memory first and only write the PNG when the image
      is new.
- [ ] **S** Create `history.bin`, its `.tmp` file and the `images/` directory with
      `0600`/`0700` permissions. They currently default to `0664`, readable by
      other accounts.
- [ ] **M** Report startup errors visibly: a bad config or hotkey currently kills
      the background process with its output going to `/dev/null`. Fall back to
      defaults and show a desktop notification (`notify-send` or `gio::Notification`).
- [ ] **S** Pass the real `AppConfig` to `ClipboardMonitor` instead of
      `AppConfig::default()` (`app.rs:157`).

## Phase 1 — Requested behaviour changes

### Move a re-copied item to the top
- [ ] **S** In `MemoryStore::add`, when a duplicate is found, remove the old entry and
      re-insert it at the newest position with a fresh `copied_at`. Keep its
      `pinned`, `label`, `color` and (new) `tag` (`store/memory.rs:23-39`).
- [ ] **S** Do the same for images (duplicate image hash).
- [ ] **S** Monitor: the early returns on `last_image_hash` and `contains_image_hash`
      skip the store entirely. Route duplicates through `add` so they get bumped
      (`monitor.rs:133-145`).
- [ ] **S** Picking an item in the popup should bump it too. Add
      `Store::touch(id)` and call it from `on_select` / `on_copy`, because the
      monitor's `last_text` check can swallow the change (`app.rs:271-298`).
- [ ] **S** Unit tests: re-adding moves to the top, keeps metadata, respects
      `max_history`.

### Scroll-to-top
- [ ] **M** Floating round "↑" button over the bottom-right of the list (a `gtk4::Overlay`
      around the `ScrolledWindow`). Show it when the scroll offset is more than about
      one row, hide it at the top, and animate the scroll.
- [ ] **S** Keyboard: `Home` / `Ctrl+Home` go to the top and select the first row;
      `End` goes to the bottom.
- [ ] **S** Reset the scroll position and selection to the top every time the
      popup opens (check whether the previous position currently carries over).

## Phase 2 — Redesign (match the mockup)

### Window / header
- [ ] **M** Dark rounded window: about 14 px radius, 1 px subtle border, drop
      shadow. Needs a transparent window background plus CSS on an inner box.
- [ ] **S** Header: app icon in a rounded square, bold title "Clipboard Manager".
- [ ] **M** Header **pin** button = "keep open" (the popup is not hidden on focus
      loss while it is on). Store it in config/state.
- [ ] **M** Header **☰ menu** (`gtk4::PopoverMenu`): Clear all · Pause capture ·
      Settings (opens config) · Reload · About · Quit.
- [ ] **S** Header round **✕** close button.
- [ ] **S** Remove the current text "Clear All" button (it moves into the menu).

### Search
- [ ] **S** Rounded search field with a magnifier icon, placeholder
      "Search clipboard…".
- [ ] **S** `Ctrl+K` keyboard-hint chip on the right; `Ctrl+K` focuses the search
      from anywhere in the popup.

### Rows
- [ ] **M** New row layout, left to right:
      `● colour dot` · `type icon tile / image thumbnail` · `title + subtitle` ·
      `tag pill` · `relative time` · `pin icon`.
- [ ] **S** **Colour dot** = the entry's colour (grey when it has none).
- [ ] **M** **Type icon tile** (rounded square, 40 px) chosen by content type:
      text `T`, document, terminal `>_`, code `</>`, link, shield (secret).
      Bundle SVG icons in the binary (`include_bytes!` + `gdk4::Texture`) so they
      look the same on every GTK theme.
- [ ] **M** **Image rows**: a small rounded thumbnail (about 74×54) replaces the
      icon tile; the subtitle is `1920 × 1080 • PNG`.
- [ ] **S** **Title** = the label if set, otherwise the detected type name ("Text",
      "URL", "Code", "Shell", "Image", "Screenshot").
- [ ] **S** **Subtitle** = one-line ellipsized preview, with newlines collapsed.
- [ ] **M** **Tag pill** (Work / Security / Snippets…): a coloured rounded badge.
      Needs a new `tag` field (see data model below).
- [ ] **S** **Pin icon** on every row: outline when unpinned, filled orange when
      pinned.
- [ ] **S** Pinned row gets an orange accent border with rounded corners.
- [ ] **S** Row hover / selection states that match the dark theme.
- [ ] **M** Decide where copy / terminal-paste / delete go (they are not in the
      mockup): show them on row hover and/or in a right-click menu. Keep the
      `Delete` key for removal.
- [ ] **S** Better relative time ("169 days ago" → "5 months ago"; singular forms).

### Theming
- [ ] **M** Ship a built-in "dark" theme matching the mockup (accent `#f97316`),
      keep "system" as an option: `theme = "dark" | "light" | "system"`.
- [ ] **S** Update `style.rs` defaults and document every colour key in
      `config/default.toml`.

### Data model (needed by the redesign)
- [ ] **M** `ClipboardEntry.tag: Option<String>` plus content-kind detection
      (URL, path, shell command, code, colour hex, email, secret-looking string).
      Detect at display time, or store it.
- [ ] **M** History format **V4** (adds the tag; keep loading V1–V3). Store the
      image format (PNG/JPEG) for the subtitle.
- [ ] **S** Right-click editor: add a tag field (free text with suggestions from
      existing tags).

## Phase 3 — New features

- [ ] **M** Tray icon: wire up the existing `src/tray.rs` (ksni) with Show / Pause
      / Quit.
- [ ] **S** `Alt+1`…`Alt+9` pastes the Nth visible item directly.
- [ ] **M** Pause capture / private mode (from the menu, the tray, or a
      `clipboard-manager pause` command).
- [ ] **M** Skip secrets: ignore clipboard offers carrying
      `x-kde-passwordManagerHint=secret`. Optional regex ignore list in config.
- [ ] **M** App exclusion list (e.g. KeePassXC, 1Password): skip capture while one
      of these windows is focused (X11 `WM_CLASS`).
- [ ] **M** Filter chips under the search field: All · Pinned · Text · Images ·
      Links · by tag.
- [ ] **M** Full-text preview on hover or `Space` (Quick Look style) for long items
      and large images.
- [ ] **S** Paste as plain text (strip formatting) action.
- [ ] **M** Auto-expire: `expire_after_days` for unpinned items.
- [ ] **M** CLI subcommands: `list`, `get <n>`, `clear`, `pause`, `resume`,
      `show` (opens the popup — good for keybinding on any DE).
- [ ] **M** Export / import history (JSON) for backup and migration.
- [ ] **L** Primary-selection (middle-click) history, opt-in.
- [ ] **L** Settings window in the GUI instead of hand-editing TOML.
- [ ] **L** Snippets: permanent, named text entries separate from history.
- [ ] **S** `popup_width`, `popup_max_items`, `show_timestamps` config keys: make
      them actually work (currently parsed and ignored).

## Phase 4 — Code quality, platform & infrastructure

- [ ] **L** Refactor `App::run` (300-line nested closures) into a `Controller`
      struct holding store, popup and platform. This also removes the
      `repop_shared` indirection and most clippy "complex type" warnings.
- [ ] **S** Replace the 50 ms hotkey poll timer with `glib::MainContext::channel`
      / `async-channel` + `spawn_local`.
- [ ] **S** Assign ids from a counter instead of scanning for the current max.
- [ ] **M** Resolve the Wayland contradiction: `main.rs:20` exits on Wayland, yet the
      README and release notes advertise support, and ~600 lines of Wayland backend
      are unreachable. Either support it properly or remove the claims and code.
      Base backend selection on the actual GDK backend, not `$WAYLAND_DISPLAY`.
- [ ] **S** Keep the popup on the monitor under the cursor (use
      `gdk4::Display::monitor_at_surface` / monitor geometry, not the whole X screen).
- [ ] **S** Use `dirs::config_dir()` for the config path (respects `XDG_CONFIG_HOME`).
- [ ] **S** Real application id: `io.github.sheheemmulakkal.ClipboardManager`.
- [ ] **S** Don't recreate `~/.config/autostart/*.desktop` on every launch (users
      can't disable autostart); the .deb already installs `/etc/xdg/autostart`.
- [ ] **S** `reload`: use a PID file or D-Bus instead of `pkill -f 'clipboard-manager$'`.
- [ ] **S** Remove dead code and dependencies: `src/paste/`, `chrono` (and `ksni`
      unless the tray is wired up), the unneeded `unsafe impl Send/Sync` for
      `WaylandPlatform`, `#[allow(dead_code)]` on `App`.
- [ ] **S** Fix the 26 clippy warnings.
- [ ] **M** Tests: persistence round-trip for V1–V4, truncated/corrupt files,
      oversize entry, search filter, sort order, move-to-top, relative time.
- [ ] **S** CI workflow on push/PR: `cargo fmt --check`, `cargo clippy -D warnings`,
      `cargo test`.
- [ ] **S** Commit the pending `Cargo.lock` version bump (1.1.0 → 1.2.0).
- [ ] **S** Update README / CHANGELOG / ARCHITECTURE / screenshots after the
      redesign.

---

## Open decisions

1. Where do copy / terminal-paste / delete actions live in the new design (hover
   buttons, right-click menu, or both)?
2. Label vs tag: keep both (label = title, tag = pill), as in the mockup?
3. Built-in dark theme as the default, or keep following the system theme by
   default?
4. Wayland: invest in real support, or declare X11-only?
