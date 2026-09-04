# litastum: stack choices

## Why crossterm + ratatui

`crossterm` + `ratatui` for the TUI: both are genuinely cross-platform
(Windows Console API + Unix terminals via one API), unlike some
alternatives in this space.

## Hard-won lesson: gate unix-only APIs

An earlier prototype (unrelated crate, `xplr`) failed to build on native
Windows because it used `std::os::unix::prelude::MetadataExt`
(`.uid()`/`.gid()`) and the `xdg` crate (XDG Base Directory spec,
Unix-only) unconditionally, with no `#[cfg(windows)]` fallback.

**Never add unix-only APIs or the `xdg` crate to this project without
gating them behind `#[cfg(unix)]` with a real Windows branch alongside.**
Use the `directories` crate for config/cache/data paths instead of `xdg`
— it resolves the right convention per OS.

## Slint rejected for the core app

Slint (a retained-mode GUI toolkit) was considered and rejected: it has
no character-grid primitive and its `TextEdit` widget has no syntax
highlighting, so it wouldn't save meaningful work over building the TUI
directly, and a windowed app loses the "runs inside any terminal / over
SSH" property that matters here.

## Built-in editor: `edtui`, with our own non-modal keymap

First tried `tui-textarea` (see history below), then switched to `edtui`
once syntax highlighting became a real requirement. `edtui` is
vim-inspired by default, but its `KeyEventHandler::new(register,
capture_on_insert)` accepts *any* binding table — it already ships
`vim_mode()` and `emacs_mode()` as two examples of this, not a hardcoded
choice — so we built our own standard (non-modal, VSCode/Windows-
convention) keymap in `editor.rs::standard_key_handler`, and get syntax
highlighting (via its `syntax-highlighting` feature, backed by
`syntect`) essentially for free. This is why the switch was worth it
over hand-rolling syntax highlighting on top of `tui-textarea` (which
has no hook for it at all — only cursor/selection/search highlighting).

**Version win**: `edtui` 0.11.x already tracks `ratatui ^0.30` /
`crossterm ^0.29`, so switching to it *undid* the downgrade
`tui-textarea` had forced (see history below) — back to current
versions, no compromise needed this time.

**Hard-won lessons from building the custom keymap** (all found by
writing tests for it, not by inspection — see `editor.rs`'s test
module):
- Selection in `edtui` is inclusive on both ends (vim-style): N
  `Shift+Right` presses from the selection start selects N+1
  characters, not N. A test asserting a 5-character selection after 5
  presses failed with a trailing extra character included.
- `capture_on_insert: false` (the vim-mode default) relies on
  `SwitchMode(Insert)` transitions to create undo checkpoints. Our
  keymap sets `state.mode = Insert` once directly at open and mostly
  stays there for plain typing, so with `false`, Ctrl+Z was a silent
  no-op — a typing session created *zero* checkpoints. Switched to
  `true` (checkpoint before every character; less granular grouping
  than an editor like VSCode manages, but `EditorState::capture` is
  crate-private so there's no hook to implement burst-grouping
  ourselves).
- `Paste` (vim's `p`) inserts *after* the cursor, not at it —
  `PasteBefore` (vim's `P`) is the one that matches standard
  paste-at-cursor behavior. Easy to pick the wrong one; the crate's own
  docs don't frame it as "the standard one vs. the vim one".
- `PasteOverSelection` (replace a selection with pasted text, i.e. what
  Ctrl+V normally does over a selection) exists internally but isn't
  publicly exported from the crate — our Ctrl+V-over-selection binding
  is a simplification (clears the selection, then pastes at the cursor,
  rather than replacing the selected text) rather than the real thing.
  See `TODO.md`.

**OS clipboard integration**: `edtui` has its own optional `arboard`
feature (on by default) that would give this for free, but its
`arboard` dependency doesn't set `default-features = false`, so
enabling it pulls in `image`/`image-data` for bitmap clipboard support
we don't need. Kept our own minimal `arboard` dependency instead
(`default-features = false`) and bridged it into `edtui`'s pluggable
`ClipboardTrait` via `editor.rs::OsClipboardBridge` — `edtui`'s own
copy/cut/paste actions then just work against the real OS clipboard
with no further plumbing.

**Known risk, not yet hit**: `edtui`'s `syntax-highlighting` feature
pulls in `syntect`, which (via its own default features) pulls in
`onig` — a C library (Oniguruma) compiled through the `cc` crate. This
built fine on the Windows dev machine this was integrated on (a C
toolchain was already present), but it's the first C-toolchain
dependency in this project, which every other choice so far has
deliberately avoided (see the pure-Rust preferences below). Revisit if
a build environment without a C toolchain (certain CI images, some
cross-compilation targets) turns out to need this crate.

### History: `tui-textarea` (superseded above)

Tried first for the F4 built-in editor: standard (non-modal) keybindings
by default, matching this project's own workflow, without needing a
custom keymap. Required downgrading `ratatui`/`crossterm` from
`0.30`/`0.29` to `0.29`/`0.28` (its exact pin, not just "close enough" —
two semver-different copies of `ratatui` can't share `Frame`/`KeyEvent`
types in one dependency graph). Its own keymap turned out to be
Emacs-style, not OS-standard — `Ctrl+C`/`Ctrl+X` happened to line up
with copy/cut, but `Ctrl+V` was bound to "scroll down a page" (Emacs
`C-v`); paste was `Ctrl+Y` in its default map, found by hand-testing
after the initial integration when paste silently did nothing useful.
Dropped once syntax highlighting became a requirement it has no hook
for at all (see above) — `edtui` covered everything `tui-textarea` did
plus that, once given a matching non-modal keymap.

## Later-stage crate choices (rationale locked in now, not yet added)

- Scripting: prefer `rhai` (pure Rust, trivially cross-compiles,
  sandboxed by default) over `mlua` (pulls in a C toolchain via
  `vendored`/`luajit`) unless Lua-language parity with Far's own macros
  becomes a real requirement.
- Live config reload: `notify` crate (cross-platform watcher).
- Archives: `zip`, `tar` + `flate2`, `sevenz-rust` — avoid `libarchive`
  C bindings.
- SFTP: `russh` (pure Rust) — avoid `ssh2`'s libssh2 C dependency.
- Previews: `ratatui-image` (falls back to Unicode halfblocks
  everywhere; native Sixel/Kitty/iTerm2 protocols are mostly
  Unix-terminal territory — don't expect Windows Terminal to get the
  high-fidelity path).
- Plugins (if ever needed): WASM (`wasmtime`/`extism`) over native
  `.dll`/`.so` loading via `libloading`.

Same cross-platform-build rationale runs through all of these: pure-Rust
or C-toolchain-free dependencies only, real Windows support, not an
afterthought `#[cfg(unix)]`-only feature.
