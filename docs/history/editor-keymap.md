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
