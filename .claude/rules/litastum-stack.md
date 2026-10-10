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

## Own window: a host terminal, not a GUI rewrite

litastum stays a console app. Its window (`gui/`, `litastum-gui`) is a
small terminal emulator that runs it in a pseudoconsole: `winit`,
`softbuffer` + `cosmic-text`, `alacritty_terminal` -- pure Rust, no GUI
framework (`egui` and Slint were both turned down; neither has a
character grid). Anything the window needs from the app goes through
the terminal, as it would in Windows Terminal; don't add window-only
code paths to the app. The one exception is a hint the terminal can't
carry through ConPTY: `LITASTUM_HOST_CELL_SIZE`, which picks iTerm2
images at the window's cell size. History: `docs/history/launching.md`.

The window has tabs, as Windows Terminal does (`gui/src/tabs.rs`,
`gui/src/app/tab_bar.rs`): each tab is its own litastum in its own
pseudoconsole, one shown at a time under a bar of them. `Ctrl+Tab`/
`Ctrl+Shift+Tab` switch (requested), `Ctrl+Shift+T` opens one,
`Ctrl+Shift+W` closes one; a click shows a tab, its `x` or a middle
click closes it, `+` opens one. Only the shown tab gets the focus
(focus reports), since every litastum polls `Ctrl+V` system-wide. A tab
is labelled with what its litastum calls itself -- the active panel's
directory or the edited file (`event_loop::sync_title`, `OSC 2`).
On the editor screens a click goes to the cell edge nearest the pointer
(`gui/src/mouse.rs::clicks_snap_to_edges`), so a letter's right half
puts the caret after it -- a terminal can only report the cell. In any
other terminal on Windows litastum does the same itself
(`src/cell_halves.rs`: the pointer's pixel position, cells learned from
its moves).
Zoom is per litastum screen -- panels, the editor, Compare, the conflict
resolver each keep their own (requested; in Windows Terminal by pressing
its zoom keys, `src/terminal_zoom.rs`, [[litastum-config]]), saved for the user in `config.json`
([[litastum-config]]): litastum names its screen with `OSC 1337 ;
SetUserVar=litastum_screen=...` (ConPTY passes it through; the window
cuts it out with the inline images, `intercept.rs`).

The console app uses the same engine the other way round: commands
typed in litastum run in a pseudoconsole of its own, parsed by
`alacritty_terminal` into the user screen (`src/user_screen/`), so
`Ctrl+O` and a running command are drawn with our own UI. See
[[litastum-command-line]].

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

**Hard-won lessons from building the custom keymap** (found by writing
tests, not by inspection):
- **Selection is inclusive on both ends (vim-style)**, and
  `SwitchMode(Visual)` alone already selects the current cell. For
  `Shift+arrows`, the fresh `Shift+Left`/`Right` table entries therefore
  don't chain a `Move`; `bindings/shift_select.rs` picks the one cell
  (`Right`: the cell under the cursor; `Left`: the cell it moves onto),
  so N presses select N characters. `Up`/`Down` keep the chained move,
  plus a one-time trim of the aligned landing column and a column
  restore (`Editor::vertical_shift_anchor`) on a round trip. Five
  real reports shaped this, including one that generalized a
  `Left`/`Right` fix to all four directions and broke `Down` -- they are
  different kinds of motion. Full record: `docs/history/shift-select.md`.
  A selection from column 0 ends on a line break (`(row, len)`), and
  deleting a selection goes through our `Editor::delete_selection`:
  `edtui`'s `DeleteSelection` removes a fully selected row, break and
  all.
- **The `Standard` keymap owns undo/redo**, not `edtui` -- see
  [[litastum-editor-undo]]. `Editor` builds the table with
  `capture_on_insert` off (`standard_key_handler(false)`): `edtui`'s
  history copied the whole buffer before every typed character, unused.
  The raw-table tests pass `true` to exercise `edtui`'s own `Undo`.
- **`Ctrl+V` is `Editor::fast_paste_from_clipboard`/`paste_text`**,
  intercepted in `Editor::input` ahead of `edtui` (speed and undo --
  see [[litastum-performance]]). The table's own `PasteBefore` entry
  (vim's `P`, inserts *at* the cursor; `Paste`/vim's `p` inserts
  *after* it) is only reached by the raw-table tests. Over a selection,
  `paste_text` replaces it: `Editor::delete_selection` first, under the
  same undo snapshot (`edtui`'s `PasteOverSelection` isn't exported).
- **`Ctrl+Shift+Left`/`Right` (word-wise selection) is hand-rolled**
  (`editor/bindings/word_select.rs`), intercepted in `editor_keymap`
  ahead of `Editor::input` -- no sequence of `edtui`'s declarative
  actions could give VS Code's behavior, and its `Selection`/
  `CharacterClass` are `pub(crate)`. Forward lands on a word's *last*
  character, backward on its *first*; `state.cursor` always stays
  exactly on the selection's live end (`edtui` assumes it everywhere --
  breaking it once cost a revert). Seventeen real reports shaped it;
  the full record of attempts and why each was reverted is
  `docs/history/word-select.md` -- read it before changing that file.
- **Plain `Left`/`Right` don't cross line boundaries in `edtui`**
  (`MoveForward`/`MoveBackward` are column-only), and a table entry
  can't say "wrap only if the move was a no-op" (`Chainable` always runs
  every link). `line_wrap.rs::wrap_line_boundary_arrow_movement` runs
  after the unmodified table and, only if the cursor didn't move, calls
  `edtui`'s own `MoveUp`/`MoveDown` + `MoveToStartOfLine`/
  `MoveToEndOfLine` -- which already extend a Visual selection, so
  `Shift+Left`/`Right` wrap for free. `Ctrl` (word-wise) is deliberately
  excluded.

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

**Bundling our own grammars for what `syntect`'s default set lacks**:
`syntect`'s bundled syntax set (sourced from sublimehq/Packages) is
missing some real-world extensions — confirmed for PowerShell and INI
(`editor.rs::BUNDLED_GRAMMARS`, see [[litastum-theming]]'s "Syntax
highlighting" section for the fix and why the first source tried for
PowerShell, github.com/PowerShell/EditorSyntax, was a dead end). The
general pattern for adding one: `syntect` only loads the YAML `.sublime-syntax`
format via `SyntaxDefinition::load_from_str` — its `plist-load` feature
covers `.tmTheme` *color themes*, not `.tmLanguage` *grammars*, so a
`.tmLanguage`-only source needs converting first (not attempted here —
found a maintained `.sublime-syntax` source instead). `SyntaxSet` isn't
`Clone`, so extending `edtui`'s own shared default set isn't possible
without reloading it entirely — cheaper to build a second, minimal
`SyntaxSet` (`SyntaxSetBuilder`) containing just the one bundled
grammar, and fall back to it only for the extensions `syntect`'s own
set doesn't resolve.

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
- Previews: **landed** for images (`F3`, `explorer/image_preview.rs` +
  `ui/image_preview.rs`) — `ratatui-image` + `image` (trimmed to
  `default-features = false` with just the `jpeg`/`png`/`bmp` decode
  features and the `crossterm` picker backend actually used, rather
  than the full default set, which pulls in heavy AV1/WebP encoders
  — `rav1e`/`ravif` — this app never needs). `App::image_picker`
  (`Picker::from_query_stdio()`, queried once in `main()` right after
  entering the alternate screen but before the main loop reads any
  keyboard events — required ordering, since the query itself
  writes/reads raw escape sequences on stdio) picks the best rendering
  protocol the real terminal actually answers for (Sixel/Kitty/iTerm2),
  falling back to `Picker::halfblocks()` only if the terminal doesn't
  respond (e.g. legacy Windows Console/ConPTY, handled internally by
  `ratatui-image` itself via a 2-second timeout, never blocking
  startup). A first version forced `Picker::halfblocks()`
  unconditionally, reasoning (without actually testing) that terminal
  capability probing would be unreliable on Windows — reported
  directly as looking unacceptably block/blurry once tried for real
  (a screenshot with small text), which is exactly the "looks fine
  everywhere but a terminal" case half-blocks can't do justice to;
  querying properly instead of assuming lets a real Sixel-capable
  terminal (Windows Terminal now included) render close to a real
  image. `.md` file preview (also `F3`, per `TODO/viewer.md`) is still
  unimplemented.
- Plugins (if ever needed): WASM (`wasmtime`/`extism`) over native
  `.dll`/`.so` loading via `libloading`.

Same cross-platform-build rationale runs through all of these: pure-Rust
or C-toolchain-free dependencies only, real Windows support, not an
afterthought `#[cfg(unix)]`-only feature.
