use std::collections::HashMap;

use crossterm::event::KeyCode;
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
pub(super) use shift_select::anchor_fresh_shift_selection;
pub(super) use word_select::extend_word_selection;

/// A non-modal (VSCode/Windows-convention) keymap for `edtui`, which
/// ships only Vim and Emacs presets. `edtui` is explicitly designed for
/// this — `KeyEventHandler::new` takes any binding table — so this
/// isn't a workaround.
///
/// The editor stays in `EditorMode::Insert` for ordinary typing/
/// movement; `EditorMode::Visual` is entered only for the duration of a
/// `Shift+Arrow` selection and always exited back to `Insert` (never
/// left in `Normal`, which this keymap doesn't otherwise use).
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
        // `Left`/`Right`'s fresh entries only switch to Visual mode --
        // no `Move` chained on top. `SwitchMode(Visual)` alone already
        // anchors a one-character selection on the current cell (vim's
        // own `v` semantics), so chaining a `Move` here used to grab a
        // *second* character on what a user experiences as a single
        // keypress (reported directly: one `Shift+Right` before "Draft"
        // selected "Dr", not "D"). See
        // `shift_select.rs::anchor_fresh_shift_selection` for the real
        // fix (which also covers why it's `Left`/`Right`-only, not
        // `Up`/`Down` -- see below) and its own call site in
        // `editor.rs::Editor::input` for how it's sequenced with the
        // line-boundary wrap check.
        //
        // `Up`/`Down` keep the original chained form, deliberately --
        // a first attempt applying the same fix to them too was
        // reported broken immediately (`Shift+Down` selecting one
        // character to the right instead of moving to the next line).
        // There's no single-character "N+1, not N" granularity to fix
        // for a row jump the way there is for `Left`/`Right`: "move to
        // the same column on the next line, selecting everything in
        // between" -- exactly what this chain already does -- was
        // always the wanted behavior.
        (i(KeyInput::shift(KeyCode::Left)), SwitchMode(EditorMode::Visual).into()),
        (i(KeyInput::shift(KeyCode::Right)), SwitchMode(EditorMode::Visual).into()),
        (i(KeyInput::shift(KeyCode::Up)), SwitchMode(EditorMode::Visual).chain(MoveUp(1)).into()),
        (i(KeyInput::shift(KeyCode::Down)), SwitchMode(EditorMode::Visual).chain(MoveDown(1)).into()),
        (v(KeyInput::shift(KeyCode::Left)), MoveBackward(1).into()),
        (v(KeyInput::shift(KeyCode::Right)), MoveForward(1).into()),
        (v(KeyInput::shift(KeyCode::Up)), MoveUp(1).into()),
        (v(KeyInput::shift(KeyCode::Down)), MoveDown(1).into()),
        // Ctrl+Shift+Left/Right (word-wise selection) are deliberately
        // *not* in this table -- `editor_keymap.rs::handle_editor_key`
        // intercepts them ahead of `Editor::input`/this whole table and
        // calls `word_select::extend_word_selection` directly instead.
        // Forward and backward each need a *different* single `edtui`
        // action (`MoveWordForwardToEndOfWord` vs. `MoveWordBackward`),
        // so nothing here actually stops this pair from moving into the
        // table too -- it stays a direct call mainly because
        // `extend_word_selection` also logs a debug line per press (see
        // its own doc comment for the real history of why the *choice*
        // of action per direction took several attempts to land on).

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
        (v(KeyInput::new(KeyCode::Backspace)), DeleteSelection.chain(exit_selection()).into()),
        (v(KeyInput::new(KeyCode::Delete)), DeleteSelection.chain(exit_selection()).into()),

        // Undo/redo (Windows/VSCode convention).
        (i(KeyInput::ctrl('z')), Undo.into()),
        (i(KeyInput::ctrl('y')), Redo.into()),

        // Clipboard. Copy/cut only make sense with a selection; paste
        // works from plain typing mode, and also exits a selection
        // first if one was active (simplification: this does not
        // replace the selection with the pasted text, just clears it
        // and pastes at the cursor — see TODO.md). `PasteBefore` (vim's
        // `P`) inserts exactly at the cursor; the plain `Paste` action
        // (vim's `p`) inserts *after* it instead, which felt wrong for
        // a "standard" editor — found while writing tests for this.
        (v(KeyInput::ctrl('c')), CopySelection.chain(exit_selection()).into()),
        (v(KeyInput::ctrl('x')), DeleteSelection.chain(exit_selection()).into()),
        (i(KeyInput::ctrl('v')), PasteBefore.into()),
        (v(KeyInput::ctrl('v')), exit_selection().chain(PasteBefore).into()),
    ]);

    // `capture_on_insert: true` -- take an undo checkpoint before every
    // typed character. `false` (the vim-mode default) relies on
    // `SwitchMode(Insert)` transitions to create checkpoints instead,
    // but this keymap sets `state.mode = Insert` once directly at open
    // and mostly stays there, so with `false` a plain typing session
    // created *zero* undo checkpoints -- Ctrl+Z was silently a no-op.
    // Found by a failing test, not by inspection. Per-character undo
    // granularity isn't as slick as grouping by typing burst, but
    // `EditorState::capture` is crate-private, so there's no hook to
    // implement that grouping ourselves.
    KeyEventHandler::new(register, true)
}


#[cfg(test)]
mod tests;
