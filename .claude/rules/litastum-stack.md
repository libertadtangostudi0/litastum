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

## Built-in editor: `tui-textarea`, and why it pins our `ratatui`/`crossterm` versions

Chose `tui-textarea` (the crate `ratatui-textarea` is usually referred to
by) over `edtui` for the F4 built-in editor: standard (non-modal)
keybindings by default, which is what this project's own workflow
expects, while `edtui`'s vim-modal editing is a real feature some future
users will want but isn't the default we need today. Not a permanent
decision — revisit if vim-mode demand shows up.

**Version constraint this created**: `tui-textarea` 0.7.0 (latest at
integration time) requires `ratatui ^0.29.0` and `crossterm ^0.28`
exactly — not just "close enough". `edtui` 0.11.x, by contrast, already
tracks `ratatui ^0.30`. Picking `tui-textarea` meant downgrading the
whole project from `ratatui 0.30`/`crossterm 0.29` to `0.29`/`0.28` to
keep a single copy of each in the dependency graph (two semver-different
copies can't share `Frame`/`KeyEvent` types, so this isn't optional).
Revisit this pin when `tui-textarea` publishes a `ratatui 0.30`-
compatible release.

**Hard-won lesson: its default keymap is Emacs-style, not OS-standard.**
`Ctrl+C`/`Ctrl+X` happen to line up with copy/cut, but `Ctrl+V` is bound
to "scroll down a page" (Emacs `C-v`) — paste is `Ctrl+Y` in its default
map. Found by hand-testing after the initial integration (paste
silently did nothing useful). Fixed by intercepting `Ctrl+V` in
`main.rs::handle_editor_key` before it reaches `TextArea::input`, and
routing it to `Editor::paste` instead — see `editor.rs`. **Any other
binding we add or rely on needs checking against `tui-textarea`'s actual
default map (`tui_textarea::textarea::TextArea::input`'s match arms),
not assumed from OS/VSCode convention.**

**OS clipboard integration**: `tui-textarea`'s copy/cut/paste only
reach its own internal, in-app-only yank buffer — nothing bridges to
the real OS clipboard on its own, so copying in the editor couldn't be
pasted into another app (or vice versa). Added `arboard` with
`default-features = false` (its default pulls in `image`/`image-data`
for bitmap clipboard support, which we don't need — text only) to
bridge `Editor::copy`/`cut`/`paste` to the OS clipboard, falling back
to the internal buffer when the OS clipboard is unavailable or empty.

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
