use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use edtui::actions::{
    Action, Chainable, CopySelection, DeleteChar, DeleteCharForward, DeleteSelection, LineBreak,
    MoveBackward, MoveDown, MoveForward, MoveHalfPageDown, MoveHalfPageUp, MoveToEndOfLine,
    MoveToStartOfLine, MoveUp, MoveWordBackward, MoveWordForward, PasteBefore, Redo, SwitchMode, Undo,
};
use edtui::events::{KeyEventHandler, KeyEventRegister, KeyInput};
use edtui::EditorMode;

mod line_wrap;
mod shift_select;
mod word_select;

pub(super) use line_wrap::wrap_line_boundary_arrow_movement;
pub(super) use shift_select::{
    anchor_fresh_shift_selection, close_selection_if_back_on_the_anchors_row, exclude_landing_column_on_fresh_vertical_selection,
};
pub(super) use word_select::extend_word_selection;

/// The non-modal (VS Code/Windows-convention) keymap. `edtui` ships only
/// Vim and Emacs presets but takes any binding table. The editor stays in
/// `Insert`; `Visual` only lasts for a `Shift`+arrow selection and always
/// returns to `Insert`, never `Normal`.
pub(super) fn standard_key_handler() -> KeyEventHandler {
    /// Exits an active selection back to plain typing. Goes through
    /// `Normal` on the way, since `SwitchMode(Insert)` alone doesn't
    /// clear `state.selection` (only `SwitchMode(Normal)` does) and
    /// the view renders whatever `state.selection` holds regardless of
    /// mode — found while testing this integration.
    fn exit_selection() -> Action {
        SwitchMode(EditorMode::Normal).chain(SwitchMode(EditorMode::Insert)).into()
    }

    let i = |key: KeyInput| KeyEventRegister::i(vec![key]);
    let v = |key: KeyInput| KeyEventRegister::v(vec![key]);

    #[rustfmt::skip]
    let register: HashMap<KeyEventRegister, Action> = HashMap::from([
        // Plain movement, typing mode.
        (i(KeyInput::new(KeyCode::Left)), MoveBackward(1).into()),
        (i(KeyInput::new(KeyCode::Right)), MoveForward(1).into()),
        (i(KeyInput::new(KeyCode::Up)), MoveUp(1).into()),
        (i(KeyInput::new(KeyCode::Down)), MoveDown(1).into()),
        (i(KeyInput::new(KeyCode::Home)), MoveToStartOfLine().into()),
        (i(KeyInput::new(KeyCode::End)), MoveToEndOfLine().into()),
        (i(KeyInput::new(KeyCode::PageUp)), MoveHalfPageUp().into()),
        (i(KeyInput::new(KeyCode::PageDown)), MoveHalfPageDown().into()),
        // Ctrl+Left/Right -- word-wise, no selection. Reported missing
        // (the command line already had this, the editor never did --
        // every other key in this table is explicitly bound, `edtui`
        // has no built-in fallback once a custom table like this one is
        // in use, so an unbound key is just a silent no-op).
        (i(KeyInput::ctrl(KeyCode::Left)), MoveWordBackward(1).into()),
        (i(KeyInput::ctrl(KeyCode::Right)), MoveWordForward(1).into()),

        // Shift+arrow starts (or extends) a selection.
        //
        // `Left`/`Right`'s fresh entries only switch to Visual mode:
        // `SwitchMode(Visual)` already anchors a one-cell selection, and
        // a chained `Move` would grab a second character per press.
        // `shift_select::anchor_fresh_shift_selection` picks the cell.
        // `Up`/`Down` keep the chained move on purpose -- a row jump has
        // no such off-by-one. History: docs/history/shift-select.md.
        (i(KeyInput::shift(KeyCode::Left)), SwitchMode(EditorMode::Visual).into()),
        (i(KeyInput::shift(KeyCode::Right)), SwitchMode(EditorMode::Visual).into()),
        (i(KeyInput::shift(KeyCode::Up)), SwitchMode(EditorMode::Visual).chain(MoveUp(1)).into()),
        (i(KeyInput::shift(KeyCode::Down)), SwitchMode(EditorMode::Visual).chain(MoveDown(1)).into()),
        (v(KeyInput::shift(KeyCode::Left)), MoveBackward(1).into()),
        (v(KeyInput::shift(KeyCode::Right)), MoveForward(1).into()),
        (v(KeyInput::shift(KeyCode::Up)), MoveUp(1).into()),
        (v(KeyInput::shift(KeyCode::Down)), MoveDown(1).into()),
        // `Ctrl+Shift+Left`/`Right` aren't here: `editor_keymap` calls
        // `extend_word_selection` directly, ahead of the table. History: docs/history/word-select.md.

        // Plain movement while a selection is active collapses it.
        (v(KeyInput::new(KeyCode::Left)), exit_selection().chain(MoveBackward(1)).into()),
        (v(KeyInput::new(KeyCode::Right)), exit_selection().chain(MoveForward(1)).into()),
        (v(KeyInput::new(KeyCode::Up)), exit_selection().chain(MoveUp(1)).into()),
        (v(KeyInput::new(KeyCode::Down)), exit_selection().chain(MoveDown(1)).into()),
        (v(KeyInput::new(KeyCode::Home)), exit_selection().chain(MoveToStartOfLine()).into()),
        (v(KeyInput::new(KeyCode::End)), exit_selection().chain(MoveToEndOfLine()).into()),
        (v(KeyInput::ctrl(KeyCode::Left)), exit_selection().chain(MoveWordBackward(1)).into()),
        (v(KeyInput::ctrl(KeyCode::Right)), exit_selection().chain(MoveWordForward(1)).into()),
        (v(KeyInput::new(KeyCode::Esc)), exit_selection().into()),

        // Editing.
        (i(KeyInput::new(KeyCode::Backspace)), DeleteChar(1).into()),
        (i(KeyInput::new(KeyCode::Delete)), DeleteCharForward(1).into()),
        (i(KeyInput::new(KeyCode::Enter)), LineBreak(1).into()),
        // No `.chain(exit_selection())` here, unlike the entries above: its
        // `SwitchMode(Insert)` takes a second undo checkpoint.
        // `Editor::input` resets mode and selection instead
        // (`is_selection_consuming_key`).
        (v(KeyInput::new(KeyCode::Backspace)), DeleteSelection.into()),
        (v(KeyInput::new(KeyCode::Delete)), DeleteSelection.into()),

        // Undo/redo. Shadowed in the app -- `Editor::input` intercepts `Ctrl+Z`/
        // `Ctrl+Y` and owns the stack (`.claude/rules/litastum-editor-undo.md`).
        // Kept for the raw-table tests (`ctrl_z_undoes_last_insert`).
        (i(KeyInput::ctrl('z')), Undo.into()),
        (i(KeyInput::ctrl('y')), Redo.into()),

        // Clipboard. Copy/cut need a selection; paste over one clears it and
        // pastes at the cursor rather than replacing it (`TODO/editor.md`).
        // `PasteBefore` (vim's `P`) inserts at the cursor; `Paste` (vim's `p`)
        // would insert after it. None chain `exit_selection()`, as above.
        (v(KeyInput::ctrl('c')), CopySelection.into()),
        (v(KeyInput::ctrl('x')), DeleteSelection.into()),
        (i(KeyInput::ctrl('v')), PasteBefore.into()),
        (v(KeyInput::ctrl('v')), PasteBefore.into()),
    ]);

    // `capture_on_insert: true` -- a checkpoint before every typed character.
    // With `false`, `edtui` only checkpoints on `SwitchMode(Insert)`, which
    // this keymap almost never goes through, so `Ctrl+Z` did nothing (found
    // by a failing test). Grouping by typing burst would need the
    // crate-private `EditorState::capture`.
    KeyEventHandler::new(register, true)
}


/// Whether `key` is one of the five visual-mode bindings above
/// (`Backspace`/`Delete`/`Ctrl+C`/`Ctrl+X`/`Ctrl+V`) that consume the
/// active selection without chaining `exit_selection()` in the table
/// itself. `Editor::input` calls this after running the table's own
/// action, to reset `state.mode`/`state.selection` back to plain typing
/// by direct field assignment -- `edtui`'s `SwitchMode(Insert)` takes an
/// undo checkpoint on every transition into Insert mode, and that second
/// checkpoint (after the real one) made the first `Ctrl+Z` look like a
/// no-op. History: docs/history/editor-keymap.md.
pub(super) fn is_selection_consuming_key(key: &KeyEvent) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    matches!(key.code, KeyCode::Backspace | KeyCode::Delete) || (ctrl && matches!(key.code, KeyCode::Char('c' | 'x' | 'v')))
}


#[cfg(test)]
mod tests;
