# Contributing to Clipboard Manager

Thanks for taking the time to contribute! This document covers everything
you need to get started.

---

## Before you start

- **For bug reports or feature requests** — open an issue first. This avoids
  duplicate work and lets us align on the approach before you write code.
- **For small fixes** (typos, obvious bugs, doc improvements) — a PR without a
  prior issue is fine.
- **For large changes** (new features, refactors, new dependencies) — please
  open an issue and wait for a green light before investing time in a PR.

---

## Development setup

**Requirements:**
- Ubuntu 22.04 or newer (GTK4 is the baseline)
- Rust stable (install via [rustup](https://rustup.rs))
- An X11 session is easiest for testing (select "Ubuntu on Xorg" at login)

**Install system dependencies:**
```bash
sudo apt install \
  libgtk-4-dev libglib2.0-dev libx11-dev libxtst-dev \
  libgdk-pixbuf-2.0-dev pkg-config build-essential
```

**Clone and build:**
```bash
git clone https://github.com/sheheemmulakkal/clipboard-manager
cd clipboard-manager
cargo build
```

**Run with debug logging:**
```bash
RUST_LOG=debug cargo run
```

Kill it between test runs:
```bash
pkill -f clipboard-manager
```

---

## Project structure

See [ARCHITECTURE.md](ARCHITECTURE.md) for a full walkthrough of the code.
The short version:

```
src/app.rs          — main wiring: GTK activation, poll loop, all closures
src/clipboard/      — entry type (ClipboardContent enum) + clipboard monitor
src/store/          — Store trait, MemoryStore, PersistentStore, binary format
src/platform/       — X11 / Wayland backend (Strategy pattern)
src/hotkey/         — X11 / Wayland hotkey backend (Strategy pattern)
src/ui/             — GTK popup window, item rows, CSS generation
```

---

## Making changes

**Check before submitting:**
```bash
cargo check          # must compile clean
cargo clippy         # no new warnings
```

There is no automated test suite. Test your change manually:
1. Build and run the app
2. Copy some text / take a screenshot
3. Press Ctrl+Alt+C — verify the popup shows correctly
4. Test the specific behaviour you changed

**For Wayland changes**, test with:
```bash
# on a Wayland session (e.g. plain GNOME, not Xorg)
cargo run
```

---

## Commit style

Use [Conventional Commits](https://www.conventionalcommits.org/):

```
feat: add X          ← new feature
fix: correct Y       ← bug fix
docs: update Z       ← documentation only
refactor: simplify W ← no behaviour change
chore: bump deps     ← maintenance
```

Keep the subject line under 72 characters. Add a body if the "why" isn't
obvious from the title.

---

## Pull request checklist

Before opening a PR:

- [ ] `cargo check` passes with no errors
- [ ] `cargo clippy` passes with no new warnings
- [ ] The change is manually tested
- [ ] Relevant docs are updated (README, ARCHITECTURE.md if applicable)
- [ ] CHANGELOG.md has an entry under `[Unreleased]`

---

## Versioning

This project follows [Semantic Versioning](https://semver.org/):

- `PATCH` — bug fixes, no API/behaviour change
- `MINOR` — new features, backward compatible
- `MAJOR` — breaking changes (rare)

Releases are cut by the maintainer by pushing a `vX.Y.Z` tag. The GitHub
Actions workflow builds the `.deb` and publishes it automatically.

---

## Code style

- Follow standard Rust idioms (`cargo clippy` is the guide).
- No `unsafe` except where already present (two small GTK `set_data` calls).
- Keep the GTK side single-threaded (`Rc<RefCell<...>>`). Only use `Arc` where
  a value must cross a thread boundary.
- Add new platform-specific code in the `Platform` trait, not inline in `app.rs`.
- Don't add dependencies without discussing first — binary size and build time matter.

---

## License

By contributing you agree that your contributions will be licensed under the
same [MIT License](LICENSE) that covers this project.
