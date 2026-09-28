# Built-in editor undo/redo -- history

Current decisions: `.claude/rules/litastum-editor-undo.md`. Code:
`src/editor/editor/undo.rs`, `Editor::input`
(`src/editor/editor/input.rs`).

## The constraint behind every attempt

`edtui::EditorState::capture()` (what records a checkpoint on `edtui`'s
own stack -- `edtui-0.11.7/src/state/undo.rs`, a whole-`Lines`-clone per
checkpoint, `max_size: 100`) is `pub(crate)`. `capture_on_insert: true`
makes `edtui` call it only for `KeyCode::Char`/`Tab` in `Insert` mode --
never for `Enter`, and never for the fast paste, which bypasses `edtui`'s
dispatch for speed (`editor-performance.md`, Round 1). So a fast paste
never had a "before the paste" boundary on `edtui`'s stack.

## Attempts

1. **One dedicated paste snapshot** (`paste_undo`/`PasteUndo`), cleared
   by any other key. Covered exactly one undo after the paste. Real
   sequence: paste, type a character or press Enter, several `Ctrl+Z`.
   The snapshot was already gone, so undo fell through to `edtui`'s
   stack, which had per-character checkpoints for the typing and
   nothing for the paste -- the pasted text was undone one character at
   a time. Reported as "если вставка была куском, отмена тоже должна
   применяться к куску".
2. **A `Vec<PasteUndo>` stack.** Fixed a narrower complaint
   (consecutive pastes each got only one level of undo), not the root
   cause: still paste-only, still cleared by any other key.
3. **`Editor` owns the whole undo/redo stack for the `Standard`
   keymap** -- the current design. Every mutating key pushes one
   snapshot, so a paste's boundary is just one more entry and survives
   later edits.

## Rejected: diffing the buffer to skip no-op snapshots

Comparing the buffer before/after each key to avoid pushing a snapshot
for a no-op would be O(buffer) per keystroke -- the kind of check
`Editor::dirty` already moved away from after a pathologically long
line made every arrow press laggy. The one known false positive
(`Ctrl+C` isn't in `can_mutate_buffer`'s navigation list) got an
explicit exclusion instead.
