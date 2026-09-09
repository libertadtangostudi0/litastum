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
- **`Ctrl+Shift+Left`/`Right` (word-wise selection) hit a real limit of
  the declarative `Action`-table approach itself**, not just another
  one-off gotcha — worth its own note since every other binding here is
  a simple table entry. `MoveWordForward`/`MoveWordBackward` land
  exactly on the first character of the adjacent word; combined with
  `edtui`'s inclusive `end = cursor` selection, extending `Right`
  visibly grabbed that character into the highlight. A corrective
  `MoveBackward(1)` chained after it fixed that but left the cursor
  sitting mid-whitespace, so the *next* press's own word-scan
  immediately re-crossed the same gap and cancelled itself back to the
  same spot — repeated presses were silently ignored. Swapping to
  `MoveWordForwardToEndOfWord` (vim's `e`, always self-skips whitespace
  and lands on the word's *last* character) fixed both of those, but
  put `Right`'s cursor on a different landing grid than `Left`'s
  (`MoveWordBackward`, start-of-word) — extending right then
  immediately retracting left no longer returned to where it grew from.
  No fixed sequence of `edtui` actions gives "repeated presses always
  progress," "never grabs an extra character," and "`Left` exactly
  undoes `Right`" all at once, because it seemed like the *visible*
  selection boundary and the *cursor* position needed genuinely
  different values in the general case.

  **A fourth attempt actually decoupled them** — hand-rolled logic
  (`bindings::extend_word_selection`) that kept the cursor on the same
  word-start grid plain `Ctrl+Left`/`Right` already use (guaranteeing
  progress and symmetry, since it's one shared grid) while computing
  the highlighted end separately: trimmed by one column whenever the
  cursor sat right of the selection's own fixed anchor (`sel.start`),
  untrimmed at or left of it. This genuinely fixed all three properties
  at once — confirmed by dedicated tests, including through a real
  punctuation run — **and was reverted anyway**: `edtui` (its own
  rendering, `Editor::cursor_screen_position`, and evidently more
  besides — though not `CopySelection`, which reads `state.selection`
  directly and turned out not to be the actual bug) assumes throughout
  that `state.cursor` sits exactly on the selection's own live end.
  Deliberately violating that left the blinking terminal cursor
  visually sitting one column away from the highlighted selection
  boundary — reported directly as "broken rendering," with copying
  appearing broken too as a downstream symptom of the resulting
  confusion about what was actually selected, not a bug in `Copy`
  itself. **Landed on**: keep `state.cursor` and the selection's own
  end *exactly* equal, always — the same invariant every other
  selection action in this codebase already honors — which means
  accepting "extends into the first character of the next word" as
  expected behavior, the same already-accepted "N+1, not N" inclusive
  quirk plain character-wise `Shift+Right` has. Turned out this *also*
  gives perfect `Right`-then-`Left` round-trip symmetry for free (both
  directions share one grid), so nothing was actually traded away by
  reverting — the decoupled version's only genuine advantage (not
  grabbing that one extra character) wasn't worth an invariant
  violation with consequences elsewhere in the crate.

  Also revealed that `edtui`'s own `Selection`/`CharacterClass` types
  are `pub(crate)` — reachable to *read* via `EditorState`'s public
  `selection: Option<Selection>` field, but not nameable to *construct*
  one ourselves, and not usable to re-derive `edtui`'s own
  word-boundary rules independently (ruled out a hand-rolled "backward
  to end of word", vim's `ge`, for the same reason — no built-in to
  reuse, and no safe way to reimplement its exact classification). Not
  expressible in `standard_key_handler`'s
  `HashMap<KeyEventRegister, Action>` at all — intercepted one layer up
  instead, in `editor_keymap.rs::handle_editor_key`, ahead of
  `Editor::input`.

  **The general lesson, worth remembering before reaching for a fourth
  attempt at anything similar**: when a library's own state has an
  implicit cross-field invariant (here, cursor == selection end) that
  isn't documented but is assumed by multiple unrelated parts of it,
  treat it as load-bearing even where it's inconvenient — breaking it
  to fix one visible symptom risks reintroducing a *worse*, less
  obvious one somewhere else in the same library's machinery.

  **A fifth, real report survived that revert**: reproducible on real
  files, pasting a word-wise selection came back with one character more
  than what looked highlighted (e.g. highlighted `"planning chat "`,
  pasted `"planning chat +"`). Traced through `edtui`'s own source
  (`view.rs`), not just its behavior: `EditorView::render` paints every
  line's spans (selection color included) first, then unconditionally
  overwrites the *cursor's own cell* with `theme.cursor_style` on top —
  `.hide_cursor()` doesn't skip that overwrite, it just changes it to
  `theme.base` instead of leaving the cell alone. Since this keymap
  keeps `state.cursor` exactly on the selection's live end (the
  invariant the fourth attempt above was reverted to preserve), that
  overwritten cell is always the *last character of an active
  selection* — it visually resets to the plain background even though
  `copy_from`/`Selection::contains` both agree it's genuinely part of
  the selection (confirmed directly: `Selection::get_selected_columns_in_row`,
  which both the plain and the syntax-highlighted rendering codepaths
  call, and `copy_from`'s own `start()..=end()` range, use the *same*
  inclusive bounds — there's no actual render-vs-copy disagreement
  inside `edtui`, the copy was always right and only the cursor-cell
  paint was wrong). Fixed on litastum's side, since `EditorTheme`'s
  `cursor_style` is a public, per-render setting: `editor.rs::view` now
  sets `cursor_style` to the same `selection_style` whenever
  `state.selection.is_some()`, and only falls back to
  `.hide_cursor()`'s plain-`base` behavior with no selection active —
  see `editor::tests::selection_end_cell_renders_with_selection_color_not_base`,
  which renders through a real `TestBackend` and checks the actual pixel
  at `cursor_screen_position()`, not just the selection's own data
  (which was never the bug).

  **Sixth: once the render/copy fix above landed, real logs (per-press
  `extend_word_selection` and per-render `editor selection render`
  debug lines, plus a `clipboard: set_text called text="..."` line
  added specifically to settle this) confirmed render and copy had been
  in agreement the whole time** — the actual remaining complaint was
  simpler than any of the above: attempt 1's landing itself (`Ctrl+Shift+
  Right` stopping on the *first character of the next word*) was never
  what was being asked for, once it could be seen clearly instead of
  through a rendering bug. Confirmed directly against the user's own
  real selection: `Ctrl+Shift+Right` twice on `"Draft architecture
  derived"` correctly copied `"Draft a"` then `"architecture d"` — every
  render and copy matched — but the actual want, stated once the
  rendering fog cleared, was `"Draft "` then `"architecture "` (each
  word plus its own trailing space, never into the next word's first
  letter). A hand-rolled scan (own three-way whitespace/word/punctuation
  classification over `state.lines`, since `edtui`'s own `CharacterClass`
  is `pub(crate)` and unreachable — see above) was built to land on "the
  last character of the current run, plus its trailing whitespace" —
  **and this too turned out to not be what was actually asked for**:
  stated directly and plainly once tested against real text, selection
  should track *only* wherever the cursor itself travels, nothing added
  on either side — no invented trailing space, same as no invented next-
  word character. The hand-rolled scan (with its "hop onto the next
  word if already sitting in trailing whitespace" and "guarantee
  progress" cases for repeat-press correctness) was deleted entirely.
  **Landed on, finally**: `edtui`'s own `MoveWordForwardToEndOfWord`
  (vim's `e`) already does exactly this, unmodified, no custom scanning
  needed at all — self-skips whitespace, then lands on the *last*
  character of a word, never on whitespace and never on the next word's
  own first character either. This is literally attempt 3 from above,
  revisited: it was rejected back then specifically for breaking
  `Right`-then-`Left` round-trip symmetry with backward's own landing
  grid (`MoveWordBackward`, first-character-of-word) — but by this point
  that symmetry had already been given up (see the backward paragraph
  below, carried over unchanged from when it was first written), so
  attempt 3's one real drawback no longer mattered, and its correctness
  (a word's own first-to-last character span, nothing more) matched
  exactly what was actually being asked for the whole time.

  **Backward is still plain `edtui` `MoveWordBackward`**, exactly as
  every attempt above used it, because it was never actually the
  reported bug: it already lands cleanly on a word's own first character
  with nothing extra grabbed. `Right` and `Left` land on two different
  grids (`MoveWordForwardToEndOfWord` stops at a word's *last*
  character, `MoveWordBackward` at its *first*) — a real round-trip
  guarantee (`Right`-then-`Left` returning to an identical column) isn't
  pursued anymore, on purpose: every earlier attempt that chased it
  either grabbed an extra character somewhere or broke rendering to get
  it, while both directions are now individually correct on their own,
  simpler terms (cursor only ever visits a word's own real boundary,
  first or last character, nothing invented). See
  `bindings.rs::extend_word_selection`'s own doc comment for the full
  blow-by-blow (six attempts in total, the fourth abandoned mid-flight
  and the sixth built then deleted in the same session once real text
  proved it wrong too) and `bindings.rs`'s test module for the coverage
  this landed with, including through a punctuation run with no
  whitespace at all.
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
- Previews: `ratatui-image` (falls back to Unicode halfblocks
  everywhere; native Sixel/Kitty/iTerm2 protocols are mostly
  Unix-terminal territory — don't expect Windows Terminal to get the
  high-fidelity path).
- Plugins (if ever needed): WASM (`wasmtime`/`extism`) over native
  `.dll`/`.so` loading via `libloading`.

Same cross-platform-build rationale runs through all of these: pure-Rust
or C-toolchain-free dependencies only, real Windows support, not an
afterthought `#[cfg(unix)]`-only feature.
