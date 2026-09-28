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
  `Shift+Right` presses from the selection start used to select N+1
  characters, not N -- `SwitchMode(Visual)` alone already anchors a
  one-character selection on the current cell (vim's own `v`
  semantics), and the table used to chain a `Move` on top of that for
  the very first press too, grabbing a second character for what a
  user experiences as one keypress. Accepted at first as an unavoidable
  side effect of reusing `edtui`'s own vim-style actions -- **fixed
  later**, once reported directly against real use (one `Shift+Right`
  selecting two characters, not one): the fresh (`i(...)`) Shift+arrow
  table entries no longer chain a `Move` at all, just `SwitchMode(Visual)`;
  a small correction pass (`bindings/shift_select.rs::anchor_fresh_shift_selection`,
  called from `Editor::input` only when this key just opened a fresh
  selection) falls back to actually performing the move only when the
  anchor cell has no real character to select at all (e.g. the append
  position past a line's own last character) -- and is skipped by
  `wrap_line_boundary_arrow_movement`'s own line-boundary check
  whenever it *did* stop on a real character, so an ordinary mid-line
  `Shift+Right` doesn't get mistaken for "hit the end of the line."
  N presses now selects exactly N characters, for every N, matching
  the project's own stated VSCode-convention keymap goal instead of
  vim's.

  **Reported broken again on retest, for the opposite direction**:
  `Shift+Left` was selecting (and copying) the character to the
  *right* of the cursor, not the left. The fix above applied the same
  "stop on the anchor cell" rule to all four directions, but the
  anchor cell (wherever the cursor already sat) is only the *correct*
  first character for a *forward* selection (`Right`) -- for a
  backward one (`Left`), the character that should be selected is
  always the one the cursor is about to move *onto*, one cell further
  back, never the one it started on. Landed on splitting `Left` from
  `Right`: `Right` keeps the anchor-only check; `Left` always performs
  its move first, then -- only if that move made real progress --
  drags the anchor (`selection.start`) to match the new cursor
  position too, collapsing the selection to exactly that one
  newly-reached cell instead of spanning old-to-new (two cells, the
  same bug shape again). See
  `bindings/shift_select.rs::anchor_fresh_shift_selection`'s own doc
  comment for the full detail, including how this still lets
  `Shift+Left` at a line's own start correctly wrap into the previous
  line via `wrap_line_boundary_arrow_movement`, same as `Shift+Right`
  already does at a line's end.

  **Reported broken a third time, for `Up`/`Down`**: the fix above was
  applied to `Up`/`Down` too, on the assumption they shared the same
  "N+1, not N" bug -- they don't. `Shift+Down` started selecting only
  one character to the right instead of moving to the next line at the
  same column, with the second press then landing one column off,
  because the fresh entry no longer performed any real row-jump at
  all. There's no single-character granularity to get right or wrong
  for a row jump the way there is for `Left`/`Right` -- "move to the
  same column on the next line, selecting everything in between" was
  already exactly what the *original*, unmodified
  `SwitchMode(Visual).chain(MoveDown(1))` chain produced, and was
  always the wanted behavior. Landed on scoping the whole fix to
  `Left`/`Right` only: their fresh table entries drop the chained
  `Move` (per above), `Up`/`Down`'s keep it, unmodified, and never
  reach `anchor_fresh_shift_selection`'s own per-direction logic at all
  (they fall through its harmless `_ => true` catch-all, since
  `wrap_line_boundary_arrow_movement` -- the only thing that return
  value gates -- never acts on `Up`/`Down` either). The general lesson,
  worth remembering before generalizing a fix to "all four directions"
  again: `Left`/`Right` and `Up`/`Down` aren't actually the same kind
  of motion just because they're both bound through `Shift+arrow` --
  one moves by character, the other by row, and a fix scoped to one
  axis's own granularity doesn't necessarily transfer to the other.

  **`Up`/`Down` turned out to have their own real bug anyway, just a
  different one.** `MoveDown`/`MoveUp` (confirmed directly from
  `edtui`'s source) only ever change `state.cursor.row` -- never
  `.col` -- so a `Shift+Down`/`Up` press genuinely does land on the
  identical column on the new row, and `edtui`'s inclusive-both-ends
  model includes whatever's there. Reported against real, aligned
  text (two lines both reading `"xxx.rs    — LATER..."`, so a word
  landed on the exact same column on both): one `Shift+Down` right
  before that word swept the *destination* line's own copy of it into
  the selection too. Fixed with the same anchor/cursor split as
  `Left`/`Right`, just applied to whichever *row* is the selection's
  own far edge instead of a single cell: for `Down`, the destination
  (`state.cursor`, kept in lock-step with `selection.end`) backs off
  one column so its own row's selected span stops right before that
  column; for `Up`, it's `selection.start` (the row the press started
  on, now the selection's bottom edge, never touched by `MoveUp`
  itself) that needs the same one-column trim -- confirmed against
  `edtui`'s own multi-line selection convention (whichever raw
  `Selection` field sits on the larger row is also an *inclusive*
  upper bound on that row's own selected span) rather than assumed
  from the single-row case, after an first attempt nudged
  `selection.start.col` the wrong direction and made it worse (one
  extra character instead of one too few). Both adjustments run only
  once, on the press that opens the selection -- `MoveDown`/`MoveUp`
  never touch `.col` themselves, so whatever this leaves it at simply
  carries forward unchanged on every further continuing press, with no
  compounding drift.

  **This broke round-trip symmetry, reported later, in a separate
  session**: `Shift+Down` then `Shift+Up` (or the reverse) no longer
  returned to an empty selection at the exact starting point -- root
  cause is the same "no compounding drift" property just described,
  looked at from the other side: the one-time column adjustment from
  the first press permanently "poisons" the column for every future
  vertical move in *either* direction, including a reversing one that's
  supposed to land exactly back on the anchor. First response was a
  full revert (`Up`/`Down` back to plain, unmodified `MoveUp`/`MoveDown`,
  losing the aligned-column exclusion above entirely) -- **and that
  itself got reported broken on the very next real-world test**: an
  aligned word got swept into the selection again, the original bug
  this whole section exists to fix. Landed on keeping *both* properties
  at once instead of picking one: `Editor` now owns a
  `vertical_shift_anchor_col: Option<usize>` field, set once (to the
  pre-trim column) alongside the adjustment above, on every fresh
  `Shift+Up`/`Down` press -- `close_selection_if_back_on_the_anchors_row`
  (`bindings/shift_select.rs`) restores `state.cursor.col` from it and
  collapses the selection the moment the cursor returns to the anchor's
  own row (unconditionally, on *every* `Up`/`Down` press, not just the
  fresh one -- `edtui`'s inclusive-both-ends model still can't represent
  an empty selection any other way). The column adjustment itself had
  nowhere to keep the pre-trim value on its own (`Up`'s own half already
  mutates `selection.start`, the only other candidate), which is why
  this needed a field on `Editor` rather than something derivable from
  `EditorState` alone.
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
  See `TODO/editor.md`.
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
- **Plain `Left`/`Right` never cross a line boundary on their own** --
  reported directly ("каретка курсора не переводится автоматически на
  следующую/предыдущую строку"). Confirmed straight from `edtui`'s
  source: `MoveForward`/`MoveBackward` are deliberately column-only,
  clamping at `max_col`/`0` and never touching `state.cursor.row` --
  this was never a misconfigured binding, the behavior simply doesn't
  exist upstream. Same declarative-table limitation as word-wise
  selection above (`Chainable` always runs every link unconditionally,
  so a table entry can't say "only wrap if the plain move was a
  no-op"). Fixed the same way: `Editor::input`
  (`editor/bindings/line_wrap.rs::wrap_line_boundary_arrow_movement`)
  runs the real table first, completely unmodified, then checks
  whether a plain/shifted `Left`/`Right` press actually moved the
  cursor -- only if it didn't (already at column 0 or the line's own
  end) does it call `edtui`'s own `MoveUp`/`MoveDown` +
  `MoveToStartOfLine`/`MoveToEndOfLine` directly. Confirmed directly
  from source that all four of those already call
  `set_selection_with_lines` themselves whenever `state.mode ==
  Visual`, exactly like every other motion action -- so reusing them
  (rather than hand-rolling the row/col change) keeps a `Shift+Left`/
  `Right` selection extending correctly across the boundary for free,
  with no selection-specific branch needed. Deliberately scoped to
  exclude `Ctrl` (word-wise movement/selection) -- not part of this
  report, left alone rather than reached for speculatively.

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
