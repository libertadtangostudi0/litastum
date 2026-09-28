# litastum: built-in editor undo/redo

History (the one-shot paste snapshot, the paste-only stack, why both
failed): `docs/history/editor-undo.md`.

## The constraint: `edtui::EditorState::capture()` is `pub(crate)`

Nothing outside `edtui` can record a checkpoint on its undo stack.
`capture_on_insert` only captures `Char`/`Tab` in `Insert` mode, never
`Enter` and never our fast paste (which bypasses `edtui`'s dispatch --
[[litastum-performance]]). Don't try to work around this through
`edtui`'s stack again.

## `Editor` owns undo/redo for the `Standard` keymap

`Editor::undo_stack`/`redo_stack: Vec<Snapshot>` (`editor/editor/undo.rs`):

- `Editor::input` intercepts `Ctrl+Z`/`Ctrl+Y` ahead of `edtui`'s
  dispatch; `edtui`'s own `Undo`/`Redo` are never reached for this
  keymap.
- One snapshot is pushed before every key that could mutate the buffer
  (`should_capture_undo_snapshot` = `can_mutate_buffer` minus `Ctrl+C`),
  and by `paste_text` itself. Enter, typing and paste each push exactly
  one boundary, so a paste undoes as one block even after later edits.
- Navigation leaves the stack alone; a new edit clears `redo_stack`.
- **No before/after buffer diff to skip no-op snapshots** -- that would
  be O(buffer) per keystroke. False positives get a cheap explicit
  exclusion instead (as `Ctrl+C` did).

`Vim` is unaffected: it keeps `edtui`'s own `capture_on_insert`/`Undo`/
`Redo`, per the "no correction pass runs for Vim" rule
([[litastum-stack]]).

## Memory: `Limits::max_paste_undo_stack`

Each `Snapshot` is a full buffer clone (the same shape `edtui`'s own
stack uses). At this project's scale ([[litastum-performance]]) the
stack is capped -- default 20, overridable in `config.json`
([[litastum-config]]) -- oldest dropped first. The name predates the
stack covering every edit, not just pastes.

## Regression coverage

`editor/editor/tests.rs::undo_treats_a_paste_as_one_block_even_after_later_edits`:
paste, type two characters, three `Ctrl+Z` -- the two characters undo
one at a time, then the whole paste in one step.
