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
