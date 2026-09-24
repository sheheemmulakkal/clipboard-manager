# Clipboard Manager v2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship v2.0.0: fix the data-loss/privacy bugs, move re-copied items to the top, add scroll-to-top, rebuild the popup to match the mockups (window, rows, context menu, tray menu), add tray / Alt+1–9 / app exclusion / preview / pause, add real Wayland (GNOME) support, and back it all with tests and CI.

**Architecture:** GTK4 (gtk4-rs 0.9) single-process daemon. All UI and store work stays on the glib main thread. Background threads (hotkey, tray, paste) talk to it through one `async_channel` of `AppEvent`s consumed by `glib::spawn_future_local`. `App::run`'s nested closures are replaced by a `Controller` (Rc) that owns store + popup + platform and handles `PopupEvent`s from the popup. The UI is split into small files: window/header, search, list/rows, context menu, editor, preview, icons, theme CSS.

**Tech Stack:** Rust 2021, gtk4 0.9 / gdk4 / glib 0.20, gdk-pixbuf (+ librsvg loader for embedded SVG icons), x11rb, ashpd 0.9 (portals), ksni 0.3 (tray), async-channel 2, sha2.

**Spec:** `TODO.md` (roadmap) + the two design mockups supplied in chat (main popup; context/tray menus) + the user's decisions:
1. Row actions: hover buttons **and** right-click menu.
2. Both label (bold title) and tag (coloured pill).
3. Dark theme by default; `theme = "system" | "light"` optional.
4. Real Wayland support.

## Global Constraints

- Git: branch `feat/v2`; commit messages have **no** `Co-Authored-By` trailer (user's global CLAUDE.md).
- History file: new format **V4**; V1, V2 and V3 must keep loading.
- Everything GTK happens on the main thread; `Rc<RefCell<>>` on the GTK side, `Arc` only across threads.
- No new heavy dependencies. Allowed additions: `async-channel`. Remove `chrono`.
- Build must pass `cargo clippy --all-targets -- -D warnings` and `cargo test` at the end.
- Config stays backwards compatible: every new key has a default; unknown old keys (`popup_max_items`, `nerd_font`) are accepted and ignored/deprecated, never a parse error.
- Default hotkey stays `ctrl+alt+c`.
- Accent colour for the dark theme: `#f97316` (orange). Colour palette names: red, orange, yellow, green, blue, purple, pink, gray. Old names map: mauve→purple, peach→orange, teal→blue.
- Default tags (from mockup): Work (green), Personal (blue), Security (purple), Ideas (pink), Snippets (orange).
- Context-menu shortcuts (from mockup): Copy `Ctrl+C`, Edit `Ctrl+E`, Delete `Delete`.
- Tray menu (from mockup): header "Clipboard Manager", Show/Hide (shows configured hotkey), Pause, Settings, Quit.
- Search placeholder: "Search clipboard…", hint chip `Ctrl + K`.

## Review Focus

1. **Re-copying an item that is pinned / labelled / tagged** → it moves to the top of the unpinned-by-time order but keeps pin, label, colour and tag. (Test in Task 2.2.)
2. **Copying from a password manager while its window is focused, then switching apps** → the secret must never be stored, even though it is still on the clipboard after the focus change. (Test in Task 5.3.)
3. **Old history files (V1–V3) and old config files** with keys like `popup_max_items = 20`, `nerd_font = true`, colour names `mauve` → load without errors, nothing lost. (Tests in Tasks 1.1, 3.2, 4.1.)
4. **Huge clipboard content** (50 MB text, 8K image) → skipped with a log line, never stored, never breaks loading of the remaining history. (Test in Task 1.1.)
5. **The popup must not close unexpectedly** while any popover (context menu, submenu, editor, preview, header menu) is open, and must still close on genuine focus loss unless "keep open" is on. (Manual check in Tasks 4.4/4.5, repeated in the final review.)

---

## File Structure (after v2)

```
src/
  main.rs              CLI dispatch (start/show/pause/resume/toggle-pause/clear/list/quit/reload), daemonize, Wayland env setup
  app.rs               App::run: builds GTK app, AppEvent channel, Controller, command-line handling
  controller.rs        NEW  Controller: owns store/popup/platform, handles PopupEvent + AppEvent
  events.rs            NEW  AppEvent (threads → main) and PopupEvent (popup → controller)
  config.rs            AppConfig (+ theme, popup_height, max_text_bytes, ignore_apps, tray_icon, expire_after_days, capture_primary …)
  paths.rs             NEW  data/config/state dirs with 0700 perms; image path helpers; hex()
  notify.rs            NEW  user-visible error notification (notify-send / log)
  tray.rs              ksni tray (wired up)
  clipboard/
    entry.rs           ClipboardEntry (+ tag), EntryMeta
    kind.rs            NEW  ContentKind detection (pure, tested)
    monitor.rs         clipboard `changed` signal based monitor, size caps, secret hint, app exclusion, pause
  store/
    mod.rs             Store trait (+ touch, set_meta, next_id, expire)
    memory.rs          MemoryStore (move-to-top dedup, id counter) + tests
    engine.rs          V4 format, skip oversize entries, 0600 file + tests
    persistent.rs      PersistentStore
  hotkey/ …            unchanged API; wayland.rs fallback order changed
  platform/
    mod.rs             Platform trait (+ active_window_class, supports_cursor_position)
    x11.rs             + active_window_class
    wayland.rs         + persisted RemoteDesktop restore token
  ui/
    mod.rs
    popup.rs           window, header, search, overlay list, undo bar, keyboard, focus-loss
    item_row.rs        row layout (dot, icon tile/thumbnail, title/subtitle, tag pill, time/hover actions, pin)
    context_menu.rs    NEW  right-click sliding menu (main / tags / colours)
    editor.rs          NEW  Edit popover (title, content for text)
    preview.rs         NEW  full preview popover
    icons.rs           NEW  embedded SVG icons → gdk::Texture (colour-aware, cached)
    style.rs           theme tokens + CSS
    format.rs          NEW  relative_time, image subtitle, title/subtitle text (pure, tested)
    filter.rs          NEW  sort + filter + chip filter (pure, tested; moved out of app.rs)
```

Deleted: `src/paste/` (dead).

---

## Milestone 0 — Setup

### Task 0.1: Branch and housekeeping

- [ ] `git switch -c feat/v2`
- [ ] Commit the pending `Cargo.lock` bump: `git add Cargo.lock && git commit -m "chore: sync Cargo.lock with v1.2.0"`
- [ ] Commit `TODO.md` and this plan: `git commit -m "docs: add v2 roadmap and implementation plan"`

### Task 0.2: Test harness script (isolated display)

**Files:** Create `scripts/dev-run.sh`

Runs the debug build inside Xephyr with its own D-Bus session and its own XDG dirs so it never touches the user's running instance, clipboard, or history.

```bash
#!/usr/bin/env bash
# Run a debug build in an isolated nested X server (Xephyr) with its own
# D-Bus session and data/config dirs. Usage: scripts/dev-run.sh [display]
set -euo pipefail
DISP="${1:-:5}"
ROOT="${CM_DEV_ROOT:-/tmp/cm-dev}"
mkdir -p "$ROOT"/{data,config,state}
pgrep -f "Xephyr $DISP" >/dev/null || { Xephyr "$DISP" -screen 900x800 -ac -br >/dev/null 2>&1 & sleep 1; }
exec env DISPLAY="$DISP" -u WAYLAND_DISPLAY \
  XDG_DATA_HOME="$ROOT/data" XDG_CONFIG_HOME="$ROOT/config" XDG_STATE_HOME="$ROOT/state" \
  RUST_LOG="${RUST_LOG:-debug}" dbus-run-session -- ./target/debug/clipboard-manager
```

Screenshot helper (used in manual checks): `ffmpeg -loglevel error -y -f x11grab -video_size 900x800 -i :5 -frames:v 1 shot.png`.

- [ ] Create the script, `chmod +x`, commit `chore: add isolated dev-run script`.

---

## Milestone 1 — Critical fixes

### Task 1.1: Persistence engine hardening (skip oversize, 0600, tests)

**Files:** Modify `src/store/engine.rs`; tests in the same file (`#[cfg(test)] mod tests`).

**Interfaces:** Produces `enum ReadOutcome { Entry(ClipboardEntry), Skipped, Corrupt }` (private); public API unchanged in this task.

- [ ] **Step 1: failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cm-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("history.bin")
    }
    fn text(id: u64, s: &str) -> ClipboardEntry { ClipboardEntry::new_text(id, s.into()) }

    #[test]
    fn roundtrip_text_image_and_meta() {
        let p = tmp("rt");
        let mut a = text(1, "hello");
        a.pinned = true; a.label = Some("L".into()); a.color = Some("red".into());
        let b = ClipboardEntry::new_image(2, [7u8; 32], 10, 20);
        let e = PersistenceEngine::new(p.clone());
        e.flush(&[&a, &b]).unwrap();
        let got = e.load();
        assert_eq!(got.len(), 2);
        assert!(got[0].pinned);
        assert_eq!(got[0].label.as_deref(), Some("L"));
        assert!(matches!(got[1].content, ClipboardContent::Image { width: 10, height: 20, .. }));
    }

    #[test]
    fn oversize_entry_is_skipped_not_fatal() {
        let p = tmp("big");
        let big = text(1, &"x".repeat(MAX_ENTRY_BYTES as usize + 1));
        let small = text(2, "after");
        PersistenceEngine::new(p.clone()).flush(&[&big, &small]).unwrap();
        let got = PersistenceEngine::new(p).load();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, 2);
    }

    #[test]
    fn truncated_file_recovers_prefix() {
        let p = tmp("trunc");
        PersistenceEngine::new(p.clone()).flush(&[&text(1, "a"), &text(2, "b")]).unwrap();
        let data = std::fs::read(&p).unwrap();
        std::fs::write(&p, &data[..data.len() - 3]).unwrap();
        assert_eq!(PersistenceEngine::new(p).load().len(), 1);
    }

    #[test]
    #[cfg(unix)]
    fn history_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let p = tmp("perm");
        PersistenceEngine::new(p.clone()).flush(&[&text(1, "secret")]).unwrap();
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
```

Also add V1/V2 fixture tests: build the bytes by hand with a helper `fn v2_bytes(entries)` that writes the V2 layout (no type byte, text only, label+color) with correct CRC, then assert `load()` returns them.

- [ ] **Step 2:** `cargo test engine` → oversize + perm tests fail.
- [ ] **Step 3: implement**
  - Text branch: when `content_len > MAX_ENTRY_BYTES`, if `pos + content_len` is inside the file, advance past content, read label/colour (and tag for V4), read CRC, return `Skipped`; else `Corrupt`.
  - `parse_file`: `Skipped` → continue; `Corrupt` → break with the existing warning.
  - `flush`: open the tmp file with `OpenOptions::new().write(true).create(true).truncate(true).mode(0o600)` (`std::os::unix::fs::OpenOptionsExt`), `f.sync_all()` before rename.
  - `load`: if the existing file mode is not 0600, `set_permissions(0o600)` (fixes existing installs).
- [ ] **Step 4:** tests pass.
- [ ] **Step 5:** commit `fix(store): skip oversize entries instead of truncating history; write history 0600`

### Task 1.2: Private data dirs + paths module

**Files:** Create `src/paths.rs`; modify `app.rs`, `clipboard/monitor.rs`, `ui/item_row.rs`, `config.rs` to use it.

**Interfaces (Produces):**
```rust
pub fn data_dir() -> PathBuf;            // $XDG_DATA_HOME/clipboard-manager, created 0700
pub fn image_dir() -> PathBuf;           // data_dir()/images, created 0700
pub fn history_file() -> PathBuf;        // data_dir()/history.bin
pub fn config_file() -> PathBuf;         // dirs::config_dir()/clipboard-manager/config.toml
pub fn state_dir() -> PathBuf;           // dirs::state_dir() or data_dir(); 0700; for log + portal token
pub fn hex(hash: &[u8; 32]) -> String;
pub fn image_path(hash: &[u8; 32]) -> PathBuf;   // image_dir()/<hex>.png
pub fn thumb_path(hash: &[u8; 32]) -> PathBuf;   // image_dir()/<hex>_thumb.png
```
`ensure_private_dir(path)` creates the dir and chmods to 0700 (also fixing existing dirs).

- [ ] Unit test `hex([0xab;32])` starts with `"abab"` and has len 64; test that `ensure_private_dir` yields mode 0700 on a temp dir.
- [ ] Replace the four duplicated `dirs::data_dir()…` computations and the three hex encoders.
- [ ] Config path now uses `dirs::config_dir()` (respects `XDG_CONFIG_HOME`; same path as before when it is unset).
- [ ] Commit `refactor: centralise paths; private data dirs`

### Task 1.3: Visible startup errors + daemon log file

**Files:** Create `src/notify.rs`; modify `main.rs`, `app.rs`, `config.rs`.

**Interfaces:** `pub fn error(summary: &str, body: &str)` — logs with `tracing::error!` and runs `notify-send -a "Clipboard Manager" -i edit-paste <summary> <body>` (ignore failure).

- [ ] `AppConfig::load()` returns `(AppConfig, Option<String>)` → on a parse error, use defaults and return the error text; `App` calls `notify::error("Config error", "<msg> — using defaults")` once GTK is up.
- [ ] Invalid hotkey: replace the `eprintln!` + `exit(1)` in the hotkey start error path with `notify::error("Hotkey not registered", …)`; the app keeps running (tray/CLI `show` still work).
- [ ] Daemonize: redirect the child's stderr to `state_dir()/clipboard-manager.log` (truncate on start) instead of `/dev/null`, and set `RUST_LOG`-less default filter `warn` so warnings land there. Keep stdout null.
- [ ] Unit test: `AppConfig::from_str("max_history = \"oops\"")` returns Err; `from_str("")` returns defaults. (Add `pub fn from_str(&str) -> Result<AppConfig>` used by `load`.)
- [ ] Commit `fix: report config/hotkey errors via notification and log file`

### Task 1.4: Event-driven clipboard monitor + capture caps

**Files:** Modify `src/clipboard/monitor.rs`, `config.rs`.

- [ ] Replace the 500 ms `timeout_add_local` poll with `clipboard.connect_changed(...)` plus one initial read at startup.
- [ ] Ignore changes where `clipboard.is_local()` (our own `set_text`/`set_texture`); the controller bumps selected items explicitly (Task 2.2).
- [ ] New config `max_text_bytes` (default `1_048_576`); texts larger are skipped with `tracing::info!`.
- [ ] Images: reject before any encoding when `w*h*4 > MAX_RAW_PIXELS` (already), and keep the save→hash→dedup path (it now runs once per change, not every 500 ms).
- [ ] Monitor now receives the real config (`&AppConfig`) — delete the `AppConfig::default()` call site.
- [ ] Manual check with `scripts/dev-run.sh`: `DISPLAY=:5 xclip -sel clip <<< one` → log shows one capture; waiting 5 s shows no further captures; copying a PNG (`xclip -sel clip -t image/png -i some.png`) → one capture, `images/` gets exactly one png + thumb.
- [ ] Commit `perf(monitor): react to clipboard changes instead of polling; cap text size`

---

## Milestone 2 — Controller refactor + requested behaviour

### Task 2.1: Events + Controller (replace nested closures)

**Files:** Create `src/events.rs`, `src/controller.rs`, `src/ui/filter.rs`; modify `app.rs`, `ui/popup.rs`, `ui/item_row.rs`, `Cargo.toml` (+`async-channel = "2"`).

**Interfaces (Produces):**
```rust
// events.rs
pub enum AppEvent {             // from any thread → main loop
    Show { prev_window: Option<u64> },
    Toggle { prev_window: Option<u64> },
    SetPaused(Option<bool>),     // None = toggle
    ClearHistory,
    OpenSettings,
    Quit,
}
pub type AppSender = async_channel::Sender<AppEvent>;

#[derive(Clone, Debug)]
pub enum RowAction {
    Paste, PasteTerminal, Copy, Remove,
    TogglePin,
    SetMeta(EntryMeta),          // label/colour/tag
    EditContent(String),
    Preview,                     // handled inside popup; never reaches controller
}
pub enum PopupEvent {            // popup → controller (main thread)
    Row(u64, RowAction),
    PasteIndex(usize),           // Alt+1..9 (0-based visible index)
    SearchChanged(String),
    ClearAll,
    Menu(MenuAction),
}
pub enum MenuAction { TogglePause, OpenSettings, About, Quit }

// ui/filter.rs  (pure)
pub fn sorted(entries: Vec<ClipboardEntry>) -> Vec<ClipboardEntry>;          // pinned first, then newest
pub fn matches(e: &ClipboardEntry, query: &str) -> bool;                     // text, label, tag, kind name, "image WxH"
pub fn visible(store: &dyn Store, query: &str, chip: Chip) -> Vec<ClipboardEntry>;
```
`Controller::new(config, store, popup, platform, sender) -> Rc<Controller>`; `Controller::handle_popup(&self, PopupEvent)`; `Controller::handle_app(&self, AppEvent)`; `Controller::refresh(&self)`.
`ClipboardPopup::set_event_handler(Rc<dyn Fn(PopupEvent)>)` (set once); `ClipboardPopup::populate(&self, entries: &[ClipboardEntry])`.

- [ ] Move `sorted_entries` / `filter_entries` into `ui/filter.rs` with tests:
```rust
#[test] fn pinned_first_then_newest() { /* ids 1(old),2(pinned),3(new) → [2,3,1] */ }
#[test] fn search_matches_label_and_tag_case_insensitive() { /* label "Resume", query "res" */ }
#[test] fn search_matches_image_dimensions() { /* query "1920" matches Image 1920x1080 */ }
```
- [ ] Hotkey/tray threads send `AppEvent` through `async_channel::Sender::send_blocking`; `glib::spawn_future_local` loop receives and calls `controller.handle_app` (removes the 50 ms timer and `show_tx` mutex).
- [ ] Popup: the seven `on_*` callback slots collapse into one `event_handler`; rows emit `PopupEvent::Row(id, action)`.
- [ ] Keep behaviour identical (manual smoke test in Xephyr: open via `xdotool key ctrl+alt+c`, search, pin, delete, clear+undo, paste into `xterm`).
- [ ] Commit `refactor: introduce Controller and event channel; drop nested closures`

### Task 2.2: Re-copied items move to the top

**Files:** Modify `src/store/mod.rs`, `store/memory.rs`, `store/persistent.rs`, `clipboard/entry.rs`, `controller.rs`.

**Interfaces (Produces):**
```rust
pub struct EntryMeta { pub label: Option<String>, pub color: Option<String>, pub tag: Option<String> }
trait Store {
    fn add(&mut self, entry: ClipboardEntry);        // dedup ⇒ move existing to newest, keep meta+pin, refresh copied_at
    fn touch(&mut self, id: u64);                     // move to newest + refresh copied_at
    fn next_id(&mut self) -> u64;                     // monotonic counter (initialised to max+1 on load)
    fn set_meta(&mut self, id: u64, meta: EntryMeta); // replaces set_label
    fn set_text(&mut self, id: u64, text: String);    // edit content (Task 4.6)
    fn get(&self, id: u64) -> Option<&ClipboardEntry>;
    …existing…
}
```

- [ ] **Failing tests** in `memory.rs`:
```rust
#[test]
fn recopy_moves_to_top_and_keeps_meta() {
    let mut s = MemoryStore::new(10, true);
    let mut a = make_text(1, "a"); a.copied_at = 100; a.label = Some("L".into()); a.pinned = true;
    s.add(a);
    let mut b = make_text(2, "b"); b.copied_at = 200; s.add(b);
    let mut again = make_text(3, "a"); again.copied_at = 300; s.add(again);
    let all = s.get_all();
    assert_eq!(all.len(), 2);
    let last = all.last().unwrap();
    assert_eq!(last.id, 1);                 // original id kept
    assert_eq!(last.copied_at, 300);        // refreshed
    assert!(last.pinned);
    assert_eq!(last.label.as_deref(), Some("L"));
}
#[test] fn recopy_image_moves_to_top() { /* same with new_image and equal hash */ }
#[test] fn touch_moves_to_top() { /* ids 1,2 → touch(1) → last is 1, copied_at >= before */ }
#[test] fn next_id_is_monotonic_after_remove() { /* add ids via next_id, remove the max, next_id still increases */ }
#[test] fn dedup_disabled_keeps_duplicates() { /* deduplicate=false → 2 entries */ }
```
- [ ] Implement; `PersistentStore` flushes after `touch`/`set_meta`/`set_text`.
- [ ] Monitor: duplicate images no longer early-return on `contains_image_hash`; they call `store.add` (which bumps). Keep the `last_image_hash` shortcut only to avoid re-hashing when the same change is delivered twice.
- [ ] Controller: `Paste`/`Copy`/`PasteTerminal` call `store.touch(id)` (clipboard change is local, so the monitor ignores it).
- [ ] Commit `feat: re-copied or re-used items move to the top`

### Task 2.3: Scroll-to-top button + Home/End + reset on open

**Files:** Modify `src/ui/popup.rs`, `ui/style.rs`, `ui/icons.rs` (arrow-up icon; if icons.rs does not exist yet use the label "↑" and switch in Task 4.2).

- [ ] Wrap the `ScrolledWindow` in a `gtk4::Overlay`; add a round `Button` (`.scroll-top-btn`, halign End, valign End, margin 14) as overlay child, hidden by default.
- [ ] `vadjustment().connect_value_changed`: visible iff `value > row_height`. Click → animate to 0 with a `gtk4::AdwAnimation`-free approach: `widget.add_tick_callback` easing over 200 ms (ease-out cubic), then select + focus row 0.
- [ ] Keys: `Home`/`Ctrl+Home` → top + select row 0; `End`/`Ctrl+End` → bottom + select last (only when the search entry is not focused, otherwise the entry handles Home/End).
- [ ] On every open (`show_*`), set the adjustment to 0 and select row 0 (fresh populate path).
- [ ] Manual check in Xephyr with 50 items: button appears after scrolling, click returns to top, Home/End work, reopen starts at top.
- [ ] Commit `feat(ui): scroll-to-top button and Home/End navigation`

---

## Milestone 3 — Data model

### Task 3.1: Content kind detection

**Files:** Create `src/clipboard/kind.rs`.

**Interfaces (Produces):**
```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentKind { Text, Url, Email, Path, Shell, Code, Color, Secret, Image, Screenshot }
impl ContentKind {
    pub fn detect(text: &str) -> ContentKind;
    pub fn title(self) -> &'static str;    // "Text", "URL", "Email", "Path", "Shell", "Code", "Colour", "Secret", "Image", "Screenshot"
    pub fn icon(self) -> Icon;             // from ui::icons (Task 4.2); until then return an &'static str name
}
```
Rules (first match wins, on trimmed text):
- `Url`: single token starting with `http://`, `https://`, `ftp://`, `file://`, or `www.` followed by a dot-domain.
- `Email`: single token matching `^[^@\s]+@[^@\s]+\.[A-Za-z]{2,}$` (hand-rolled check, no regex crate).
- `Color`: `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb(…)`, `rgba(…)`, `hsl(…)`.
- `Path`: single line starting with `/`, `~/`, `./`, `../` and no spaces except escaped, or matches `^[A-Za-z]:\\`.
- `Shell`: single line starting with `$ ` or `sudo `, or first word in a known list (`cd ls cat grep find git cargo npm apt docker ssh curl wget mkdir rm cp mv chmod echo export make python3 pip systemctl journalctl kubectl`) and contains a space.
- `Secret`: single token, 20–128 chars, no spaces, at least 3 of {lower, upper, digit, symbol} **and** not Url/Path/Email; or UUID-like / hex ≥ 32 chars / starts with `ghp_`, `sk-`, `xox`, `AKIA`, `-----BEGIN`.
- `Code`: multi-line or single line containing two or more of `{ } ; => :: fn  def  class  import  use  let  const  #include  </` .
- else `Text`.

- [ ] Table-driven test:
```rust
#[test]
fn detects_kinds() {
    use ContentKind::*;
    for (s, k) in [
        ("https://github.com/x/y", Url), ("www.example.com", Url),
        ("me@example.com", Email), ("#f97316", Color), ("rgb(1, 2, 3)", Color),
        ("/usr/bin/env", Path), ("~/notes.txt", Path),
        ("git status", Shell), ("$ cargo build --release", Shell),
        ("use std::collections::HashSet;", Code), ("fn main() {\n}", Code),
        ("ghp_abcdefghijklmnopqrstuvwxyz0123", Secret),
        ("f9c0e003-3d16-4858-a405-e41ca0d1c2b3", Secret),
        ("analyze this project. I need improvements", Text), ("hello", Text),
    ] { assert_eq!(ContentKind::detect(s), k, "{s}"); }
}
```
- [ ] Commit `feat: detect clipboard content kind`

### Task 3.2: Tag field + V4 history format

**Files:** Modify `clipboard/entry.rs` (+`tag`), `store/engine.rs`.

- V4 = V3 layout + after colour: `has_tag(1) [tag_len(4) tag(n)]`, CRC still covers from id to end of tag. `VERSION = 4`; loader accepts 1–4.
- [ ] Tests: V4 roundtrip with tag; a hand-built V3 byte fixture (text + image) still loads with `tag == None`.
- [ ] Commit `feat(store): history format v4 with tags`

---

## Milestone 4 — Redesign

### Task 4.1: Theme tokens + config

**Files:** Modify `config.rs`, `ui/style.rs`, `config/default.toml`.

- New keys: `theme = "dark"` (`"dark" | "light" | "system"`), `popup_height = 560`, `popup_width = 440` (now used), `show_timestamps` (now used). `popup_max_items` and `nerd_font` are still accepted but ignored (documented as deprecated).
- `ColorConfig` keeps all existing keys (overrides win over the theme).
- `Theme` struct with resolved tokens: `bg, surface, surface_hover, border, text, text_muted, accent, danger, selection, shadow` — dark (`#16181d`, `#1f2229`, `#262a32`, `rgba(255,255,255,0.08)`, `#e8eaed`, `#9aa0a6`, `#f97316`, `#ef4444`, …), light, system (GTK `@theme_*` names).
- Palette: `pub const COLORS: [(&str, &str); 8]` (red `#ef4444`, orange `#f97316`, yellow `#eab308`, green `#22c55e`, blue `#3b82f6`, purple `#a855f7`, pink `#ec4899`, gray `#9ca3af`) and `pub fn normalize_color(name) -> Option<&'static str>` mapping legacy names.
- Default tags: `pub const DEFAULT_TAGS: [(&str, &str); 5]` = Work/green, Personal/blue, Security/purple, Ideas/pink, Snippets/orange; `pub fn tag_color(tag) -> &'static str` (default mapping, else stable hash into COLORS).
- [ ] Tests: legacy config (`popup_max_items = 20`, `nerd_font = true`, `[colors] accent = "#123456"`) parses; `normalize_color("mauve") == Some("purple")`; `tag_color("Work") == "green"`; `tag_color("zzz")` is stable across calls.
- [ ] Commit `feat(ui): theme tokens, palette and default tags`

### Task 4.2: Embedded icons

**Files:** Create `src/ui/icons.rs`, `assets/icons/*.svg` (hand-drawn 24×24 stroke icons, `stroke="currentColor"`, stroke-width 1.8, round caps): `file-text, type, terminal, code, link, shield, mail, folder, palette, image, pin, pin-filled, menu, x, search, copy, trash, arrow-up, eye, pencil, tag, pause, play, settings, power, chevron-right, chevron-left, plus, clipboard, keyboard`.

**Interfaces:**
```rust
#[derive(Clone, Copy)] pub enum Icon { FileText, Type, Terminal, Code, Link, Shield, Mail, Folder, Palette, Image, Pin, PinFilled, Menu, X, Search, Copy, Trash, ArrowUp, Eye, Pencil, Tag, Pause, Play, Settings, Power, ChevronRight, ChevronLeft, Plus, Clipboard, Keyboard }
pub fn texture(icon: Icon, color: &str, px: i32) -> gdk4::Texture;   // cached by (icon,color,px)
pub fn image(icon: Icon, color: &str, px: i32) -> gtk4::Image;       // Image::from_paintable + pixel_size
```
Implementation: `include_str!` each SVG, replace `currentColor` with `color`, load through `gdk_pixbuf::PixbufLoader::with_type("svg")` at `px*scale` (scale = display's max monitor scale factor), `Texture::for_pixbuf`. Cache in a `thread_local! RefCell<HashMap>`.
- `packaging`: add `librsvg2-common` to `.deb` depends.
- [ ] Test (needs no display): `svg_for(Icon::Pin, "#fff")` contains `#fff` and no `currentColor`; every icon's SVG parses via `PixbufLoader` (gdk-pixbuf works headless).
- [ ] Commit `feat(ui): embedded colour-aware SVG icons`

### Task 4.3: Window, header, search

**Files:** Modify `ui/popup.rs`, `ui/style.rs`.

- Window: transparent background; inner `.popup-card` box: radius 14px, 1px border, shadow `0 12px 32px rgba(0,0,0,.45)`, 10px margin for the shadow (on non-composited X the margin is dropped: check `display.is_composited()`).
- Header (`WindowHandle`): `[icon tile Clipboard] "Clipboard Manager"  … [paused badge] [pin] [☰] [✕]`.
  - pin = keep open (toggle; `.header-btn.active` in accent colour; state in `Cell<bool>` consulted by the focus-loss handler).
  - ☰ opens a popover menu: Pause/Resume capture, Clear all…, Settings, About, Quit (→ `PopupEvent::Menu` / `ClearAll`).
  - ✕ hides.
- Search: rounded field `.search-box` = [search icon][SearchEntry flat "Search clipboard…"][chip "Ctrl + K"]. `Ctrl+K` / `Ctrl+F` / `/` focus it. Esc clears then closes (existing).
- Remove the old text "Clear All" button.
- [ ] Manual check (Xephyr screenshot) against mockup 1; clicking pin keeps the window open when focusing another window; ✕ hides; menu items work.
- [ ] Commit `feat(ui): new window chrome, header actions and search box`

### Task 4.4: Rows

**Files:** Modify `ui/item_row.rs`; create `ui/format.rs`.

**Interfaces (format.rs, pure + tested):**
```rust
pub fn relative_time(copied_at: u64, now: u64) -> String;   // "just now", "5 min ago", "1 hour ago", "2 hours ago", "yesterday", "3 days ago", "5 months ago", "2 years ago"
pub fn title(e: &ClipboardEntry, kind: ContentKind) -> String;     // label or kind.title()
pub fn subtitle(e: &ClipboardEntry) -> String;              // one line, whitespace collapsed, ≤ 120 chars; images "1920 × 1080 • PNG"
```
Tests cover singular/plural and each boundary (59 s, 60 s, 3599 s, 3600 s, 86399 s, 86400 s, 30 d, 365 d), and subtitle collapsing `"a\n\n  b\tc"` → `"a b c"`.

Row layout (`.item-row`, min-height 58):
`[● dot 8px, entry colour or gray] [tile 40×40 radius 10 with kind icon | image thumbnail 64×44 radius 8] [vbox: title (bold) / subtitle (muted, ellipsized)] [tag pill?] [Stack: time label | hover actions (copy, terminal (text only), preview, delete)] [pin button: outline or filled accent]`.
- Hover: `EventControllerMotion` enter/leave switches the stack page (crossfade 120 ms). Keyboard selection does **not** show hover actions.
- Pinned row: `.item-row.pinned` accent border 1.5px, radius 12, faint accent background.
- `show_timestamps = false` hides the time label (actions still appear on hover).
- Screenshot detection: image kind is `Screenshot` when its size equals any monitor's geometry × scale.
- Tooltip on the subtitle: first 600 chars / 12 lines of the full text.
- [ ] Manual check (Xephyr screenshot) vs mockup 1 with fixture data (seed history via `xclip` with URL, code, shell, secret, image).
- [ ] Commit `feat(ui): redesigned rows with kind icons, tags and hover actions`

### Task 4.5: Context menu (right-click) with submenus

**Files:** Create `ui/context_menu.rs`; modify `item_row.rs`, `popup.rs`.

**Interfaces:** `pub fn show(row: &ListBoxRow, entry: &ClipboardEntry, all_tags: Vec<String>, suppress: Rc<Cell<u32>>, emit: Rc<dyn Fn(RowAction)>)`

- One `Popover` containing a `Stack` (slide-left/right) with pages:
  - **main**: Copy `Ctrl+C` · Edit `Ctrl+E` · Pin/Unpin · Add label ▸ · Change colour ▸ · (sep) · Paste to terminal (text) · Preview `Space` · (sep) · Delete `Delete` (danger colour).
  - **tags**: "‹ Add label" back header; DEFAULT_TAGS ∪ tags in use, each with its colour dot and a check on the current tag; "Remove label" when one is set; "+ New label…" → inline `Entry` in the page, Enter applies.
  - **colours**: "‹ Change colour"; the 8 colours with dots, check on the current; "None".
- Each item is a flat `Button` with `[icon][label][accel dim label]`, `.menu-item`; Delete `.menu-item.danger`.
- Keyboard shortcuts work on the selected row even without the menu: `Ctrl+C` (when the search entry has no text selection), `Ctrl+E`, `Delete`, `Space` (preview; when not typing in search), `Ctrl+P` toggle pin.
- The suppress-close counter increments on popup and decrements one idle tick after `closed` (existing pattern) — shared helper `popup::track_popover(&Popover, &Rc<Cell<u32>>)` used by every popover.
- [ ] Manual check: right-click → submenus navigate, tag applied shows pill, colour applied shows dot, window never closes while the menu is open, Esc closes the menu only.
- [ ] Commit `feat(ui): right-click context menu with label and colour submenus`

### Task 4.6: Edit popover

**Files:** Create `ui/editor.rs`.

- Opens from menu "Edit" / `Ctrl+E`. Fields: Title (Entry), Content (TextView in a ScrolledWindow, text entries only, monospace, 8 lines high). Buttons Cancel / Save. `Ctrl+Enter` saves.
- Save → `RowAction::SetMeta` for title (keeps colour/tag) and `RowAction::EditContent(text)` when the content changed. `Store::set_text` refuses empty text (no-op) and re-runs dedup (if the edited text equals another entry's, the other entry is removed).
- [ ] Unit test for `set_text` dedup behaviour in memory.rs.
- [ ] Commit `feat(ui): edit title and content`

---

## Milestone 5 — Features

### Task 5.1: Tray icon

**Files:** Modify `tray.rs`, `main.rs` (`mod tray`), `app.rs`, `config.rs` (`tray_icon = true`).

- ksni 0.3 blocking API (`ksni::blocking::TrayMethods::spawn`), tray struct holds `AppSender` + `paused: bool` + `hotkey_hint: String`.
- Menu (mockup): disabled header item "Clipboard Manager" with icon `edit-paste` · separator · "Show / Hide  (Ctrl+Alt+C)" → `AppEvent::Toggle` · Pause/Resume (checkmark) → `SetPaused(None)` · Settings → `OpenSettings` · separator · Quit → `Quit`. Left click (activate) → Toggle.
- Controller pushes pause state back with `handle.update(|t| t.paused = p)`.
- If no StatusNotifier host exists (GNOME without the AppIndicator extension) log info and continue.
- [ ] Manual check on the real session bus (dev profile, see Task 7.1): icon appears, each item works.
- [ ] Commit `feat: system tray icon`

### Task 5.2: Pause capture

**Files:** `controller.rs`, `monitor.rs`, `popup.rs`, CLI in Task 6.1.

- `paused: Rc<Cell<bool>>` shared with the monitor; changes while paused are marked seen (last text/hash updated) so resuming does not capture the paused content.
- Header shows a "Paused" badge; menu label flips; tray check updates.
- [ ] Commit `feat: pause/resume clipboard capture`

### Task 5.3: Secrets + ignored apps

**Files:** `monitor.rs`, `platform/{mod,x11,wayland}.rs`, `config.rs`.

- Skip when the clipboard formats contain `x-kde-passwordManagerHint` (KeePassXC, KWallet, others).
- `Platform::active_window_class(&self) -> Option<String>` — X11: `_NET_ACTIVE_WINDOW` → `WM_CLASS` (instance + class, lowercase). Wayland: `None`.
- Config `ignore_apps = ["keepassxc", "org.keepassxc.keepassxc", "1password", "bitwarden", "enpass", "gnome-keyring", "seahorse", "kwalletmanager"]` (case-insensitive match on instance or class).
- When skipped: update `last_text` / `last_image_hash` so the content is never captured later (Review Focus #2).
- Pure helper `fn is_ignored(class: &str, list: &[String]) -> bool` + tests (case-insensitive, instance or class, empty list).
- [ ] Manual check: `xterm -class KeePassXC` focused, run `xclip` inside it → not captured; switch focus → still not captured.
- [ ] Commit `feat: never capture password-manager secrets or ignored apps`

### Task 5.4: Alt+1…9 / 1…9 quick paste

**Files:** `popup.rs`, `controller.rs`, `item_row.rs`.

- `Alt+1..9` always; plain `1..9` when the search entry is not focused → `PopupEvent::PasteIndex(n-1)` (index into currently visible rows, skipping the "No matches" placeholder).
- While `Alt` is held, the first nine rows show a small number badge over the colour dot (`.quick-index`), hidden on release.
- [ ] Commit `feat: Alt+1–9 quick paste`

### Task 5.5: Full preview

**Files:** Create `ui/preview.rs`.

- Opened by `Space`, the hover eye button, or the menu. A popover anchored to the row, 400×320 max: text → monospace selectable `Label` (wrap, first 200 000 chars, "… N more characters" footer) inside a `ScrolledWindow`, header line "Text · 1,234 chars · 42 lines"; image → `gtk4::Picture` of the full PNG scaled to fit, header "1920 × 1080 • PNG · 1.2 MB". Buttons: Copy, Paste.
- `Esc` / `Space` closes.
- [ ] Commit `feat: full content preview`

### Task 5.6: Filter chips

**Files:** `ui/filter.rs`, `popup.rs`.

- A horizontal scrollable row under search: All · Pinned · Text · Images · Links · Code · (each tag in use). One active at a time. `Chip` enum `{ All, Pinned, Text, Images, Links, Code, Tag(String) }`.
- Tests in filter.rs for each chip.
- [ ] Commit `feat(ui): filter chips`

### Task 5.7: Auto-expire

**Files:** `config.rs` (`expire_after_days = 0` = off), `store` (`fn expire_older_than(&mut self, cutoff: u64) -> usize`, unpinned only), controller (on start + hourly timer).
- [ ] Test: pinned entries survive, older unpinned removed, newer kept.
- [ ] Commit `feat: optional auto-expire for unpinned items`

---

## Milestone 6 — CLI + Wayland

### Task 6.1: CLI subcommands via GApplication command line

**Files:** `main.rs`, `app.rs`.

- `ApplicationFlags::HANDLES_COMMAND_LINE`. The primary instance handles `connect_command_line`; a second invocation forwards argv over D-Bus and prints what the primary prints.
- Commands: (none) start daemon / show if running · `show` · `toggle` · `pause` · `resume` · `toggle-pause` · `clear` (unpinned) · `list [--limit N]` (index, kind, preview) · `quit` · `reload` (quit + start) · `--help` · `--version`.
- `show|toggle|pause|…` when no daemon is running: print "clipboard-manager is not running (start it with: clipboard-manager)" and exit 1 — except `show`, which starts the daemon.
- `reload` no longer uses `pkill -f`.
- Pure `fn parse_cli(args: &[String]) -> Result<Command, String>` + tests.
- Update the man page and README.
- [ ] Commit `feat: CLI subcommands routed to the running instance`

### Task 6.2: Wayland support (GNOME first)

**Files:** `main.rs`, `platform/mod.rs`, `platform/wayland.rs`, `hotkey/mod.rs`, `hotkey/wayland.rs`, `ui/popup.rs`, README.

Design:
- Remove the Wayland exit guard. On a Wayland session (`WAYLAND_DISPLAY` set) with `DISPLAY` available, set `GDK_BACKEND=x11` for our own process before GTK init: under XWayland the app can read the clipboard at any time (native Wayland clients only receive it while focused), and it can position its own window. Without `DISPLAY` (no XWayland) run native Wayland and log that background capture only works on compositors with the data-control protocol (not implemented in this version).
- `platform::detect` keys on the **session** (`XDG_SESSION_TYPE`/`WAYLAND_DISPLAY`), not the GDK backend: Wayland sessions get `WaylandPlatform` (paste via RemoteDesktop portal, no previous-window capture, no cursor query → popup centred).
- RemoteDesktop portal: persist the restore token (`state_dir()/portal-restore-token`) and pass it to `select_devices`, so the permission dialog appears once ever, not once per login.
- Hotkey fallback order on Wayland: GlobalShortcuts portal → (GNOME) gsettings custom shortcut running `clipboard-manager toggle` → evdev (input group). The gsettings binding is updated when the configured hotkey changes and removed by `postrm`.
- Popup activation: the shortcut launches `clipboard-manager toggle`; GApplication passes the activation token/startup id so `present()` is allowed to take focus.
- [ ] Unit test: `to_portal_trigger("ctrl+alt+c") == Some("<Ctrl><Alt>c")`; `to_gsettings_binding` equivalent.
- [ ] Manual check: nested `dbus-run-session -- gnome-shell --nested --wayland` if it runs here; otherwise ask the user to log into "Ubuntu" (Wayland) and run through the checklist in README → "Wayland".
- [ ] Commit `feat: Wayland (GNOME) support via XWayland + portals`

---

## Milestone 7 — Quality, CI, docs

### Task 7.1: Dev profile, cleanup, clippy

- `CLIPBOARD_MANAGER_PROFILE=dev` → app id `io.github.sheheemmulakkal.ClipboardManager.Dev`, separate data/config dirs; production id `io.github.sheheemmulakkal.ClipboardManager` (update `.desktop` files).
- Autostart: only write `~/.config/autostart` when the system autostart file `/etc/xdg/autostart/clipboard-manager.desktop` is absent **and** the user file was never created before (marker in state dir).
- Delete `src/paste/`, remove `chrono`; remove unnecessary `unsafe impl Send/Sync` and `#[allow(dead_code)]`.
- Multi-monitor placement: clamp to the monitor containing the cursor (`display.monitors()` geometry).
- `cargo clippy --all-targets -- -D warnings` clean; `cargo fmt` **not** applied wholesale (the code uses hand-aligned columns) — only new files.
- [ ] Commit `chore: cleanup, dev profile, clippy clean`

### Task 7.2: CI

- `.github/workflows/ci.yml` on push/PR: install the apt deps, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo build --release`.
- [ ] Commit `ci: build, lint and test on every push`

### Task 7.3: Docs + version

- README (features, shortcuts table, Wayland section, config keys), ARCHITECTURE (controller/events, V4), CHANGELOG `2.0.0`, man page, `config/default.toml` (every key documented), Cargo version `2.0.0`, deb description (no longer "requires X11").
- Screenshot of the new UI into README if a clean one can be produced from the dev run.
- [ ] Commit `docs: v2.0.0`

### Task 7.4: Final review + fixes

- Fresh reviewer (most capable model) reviews the whole `feat/v2` branch against this plan and the Review Focus list; fix confirmed findings; full manual checklist in Xephyr; `cargo test`, clippy.

---

## Deferred (not in this plan — call out to the user)

- GUI settings window (L), snippets library (L), primary-selection history (L), export/import (M), native wlr/ext-data-control clipboard monitor for non-GNOME Wayland compositors (L).
- "Paste as plain text": history only stores plain text already, so there is nothing to strip — dropped.
