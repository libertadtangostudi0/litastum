# Editor key dispatch (`editor_keymap`) -- history

Code: `src/editor/editor_keymap/mod.rs`. Related:
`docs/history/word-select.md`, `docs/history/shift-select.md`.

- **`F10` in the editor crashed the process.** Every non-editor-command
  key was forwarded to `Editor::input`, and `edtui` 0.11.7's
  `KeyCode::from(crossterm::event::KeyCode)`
  (`src/events/key/input.rs`) matches only fourteen variants -- `Char`,
  `Enter`, `Esc`, `Backspace`, `Delete`, `Tab`, the arrows, `Home`,
  `End`, `PageUp`, `PageDown` -- and hits `unimplemented!()` for the
  rest. `edtui_supports_key` is an allow-list of exactly those
  (a block-list would have to track every crossterm variant); anything
  else is ignored. This is also why `F9` could become the editor's menu
  key and `F3` its find-next: they were silent no-ops.
- **`Ctrl+S` sometimes did nothing**: some terminal/backend combinations
  report the Caps-Lock case even with `Ctrl` held (`Char('S')`). Letter
  shortcuts match both cases.
- **Word selection ran under Vim.** `WordSelect` called the hand-rolled
  `extend_word_selection` unconditionally, found by testing Vim mode:
  `Ctrl+Shift+Right` forced `Visual` outside any Vim binding. It's now
  forwarded as a raw key under Vim, where `vim_mode()` has no binding
  for it.
- **`Up`/`Down` in the `Ctrl+F` box** were first used for next/previous
  match and reported wrong: they browse the search history, like a
  shell. Next/previous is `Enter`/`Shift+Enter` (VS Code), and `F3`/
  `Shift+F3` from the text.
- **Search history isn't saved when the box closes**: that handler is
  heavily unit-tested and would write a real history file into the cwd
  from every test. It's saved once at clean exit (`main.rs`), the same
  memory/disk split `command_line::history` uses.

## `Left`/`Right` crossing line boundaries (`line_wrap.rs`)

Reported missing: `Left` at a line's start (or `Right` at its end) just
stayed put. `edtui`'s `MoveBackward`/`MoveForward` are column-only by
design, so no binding was misconfigured -- it was never implemented.
Another table entry couldn't do it: `Chainable` runs every link, so
"then go to the previous line's end" would fire on every `Left`. Solved
like word selection: run the unmodified table, then correct only if the
cursor didn't move. Reusing `edtui`'s `MoveUp`/`MoveDown` +
`MoveToStartOfLine`/`MoveToEndOfLine` extends a `Visual` selection for
free. `Ctrl` (word-wise) was left out: not part of the report.

The `Ctrl` guard's test calls the correction directly: an earlier
version drove it through the table and failed, because `edtui`'s
`MoveWordBackward` already crosses lines on its own.

## Search box and Vim notes

- **`Esc` closing the search box** first landed the cursor *on* the
  match's last character. Reported: "lso" looked like the cursor stopped
  between 's' and 'o'. With no selection there's no bar-cursor shift
  (`editor-rendering.md`), so it now goes one past the match.
- **Vim `Right` "stops" partway** on `.editorconfig`'s "root = true"
  (asked, with a screenshot, whether it's an `edtui` bug). It isn't:
  `Normal`-mode `l` is clamped to the last character (`max_col_normal`)
  and never wraps, as in real Vim.
- **`PasteBefore`, not `Paste`**, for the raw table's `Ctrl+V`: vim's
  `p` inserts *after* the cursor, which felt wrong in a standard
  editor -- found while writing tests.

## `Editor::input` corrections (Standard keymap)

- **`Ctrl+A`, `Backspace`, `Ctrl+Z` needed two `Ctrl+Z` presses.** The
  table entry `DeleteSelection.chain(exit_selection())` captured an undo
  checkpoint twice: once inside `DeleteSelection` (correct), then again
  when `exit_selection()`'s `SwitchMode(Insert)` re-entered Insert mode
  -- `edtui` captures unconditionally on that transition, with no opt-out
  from outside. The second checkpoint held the already-deleted state, so
  the first `Ctrl+Z` looked like a no-op. The five selection-consuming
  keys (`Backspace`, `Delete`, `Ctrl+C/X/V`) now reset mode and selection
  by direct field assignment (`is_selection_consuming_key`). (Undo for
  `Standard` later moved to `Editor`'s own stack entirely --
  `editor-undo.md`.)
- **Typing over a selection dropped the character** (`Ctrl+A` then a
  letter: nothing happened). A plain `Char` has no `Visual`-mode binding
  in the table, and `edtui`'s "typing inserts" fallback only works in
  Insert mode, so the key reached neither. It's handled ahead of
  dispatch now; the selection-consuming path couldn't host it, since
  that only clears the selection after a real binding already ran.
- **`hjkl` navigation re-checked `dirty`** on every press in Vim mode,
  missing the arrow-key exemption (`editor-performance.md`) on a
  pathologically long line. `h`/`j`/`k`/`l` in Normal/Visual bind to the
  same move actions as the arrows and never appear inside a multi-key
  sequence (checked against `vim_keybindings()`), so they're exempt too.
  `w`/`b`/`e`/... aren't: `w` also completes `dw`/`cw`, and a per-key
  check can't see `edtui`'s pending sequence.

## `Ctrl+Up`/`Ctrl+Down`: blocks, and levels in data files

Blocks are runs of non-blank lines (`editor/editor/block_move.rs`), as
in Vim's paragraph moves. Reported on a JSON file: with no blank lines
it was one block, and the caret went from the top straight to the end.
In data formats (JSON, YAML, XML-like; `NESTED_DATA_EXTENSIONS`) the
keys go by levels instead, as in a tree: down to the next line as deep
or shallower (the next key, its nested lines passed over; after the
last one, the next one up), up to the previous one (from the first
key, its parent); blank lines and lines that only close (`}`, `],`,
`</a>`) aren't stops. Code keeps blank-line blocks -- by levels, a
function body would go line by line.
