use std::collections::HashMap;

use crossterm::event::KeyCode;
use edtui::actions::{
    Action, Chainable, CopySelection, DeleteChar, DeleteCharForward, DeleteSelection, LineBreak,
    MoveBackward, MoveDown, MoveForward, MoveHalfPageDown, MoveHalfPageUp, MoveToEndOfLine,
    MoveToStartOfLine, MoveUp, MoveWordBackward, MoveWordForward, PasteBefore, Redo, SwitchMode, Undo,
};
use edtui::events::{KeyEventHandler, KeyEventRegister, KeyInput};
use edtui::EditorMode;

mod word_select;

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
        (i(KeyInput::shift(KeyCode::Left)), SwitchMode(EditorMode::Visual).chain(MoveBackward(1)).into()),
        (i(KeyInput::shift(KeyCode::Right)), SwitchMode(EditorMode::Visual).chain(MoveForward(1)).into()),
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
mod tests {
    use edtui::clipboard::InternalClipboard;
    use edtui::{EditorEventHandler, EditorMode, EditorState, Lines};

    use super::*;
    use crate::test_support::{ctrl_code_key, ctrl_key, key, shift_key};

    /// Builds an `EditorState` + our custom keymap directly (bypassing
    /// `Editor::open`'s real-file / real-OS-clipboard setup) with
    /// `InternalClipboard`, so copy/cut/paste tests never touch the
    /// actual system clipboard -- that would be flaky in CI and rude to
    /// whatever the developer running the tests had copied.
    fn test_state(contents: &str) -> (EditorState, EditorEventHandler) {
        let mut state = EditorState::new(Lines::from(contents));
        state.mode = EditorMode::Insert;
        state.set_clipboard(InternalClipboard::default());
        (state, EditorEventHandler::new(standard_key_handler()))
    }

    #[test]
    fn typing_inserts_characters() {
        let (mut state, mut handler) = test_state("");
        handler.on_key_event(key(KeyCode::Char('h')), &mut state);
        handler.on_key_event(key(KeyCode::Char('i')), &mut state);
        assert_eq!(String::from(state.lines.clone()), "hi");
    }

    #[test]
    fn shift_right_starts_a_selection() {
        let (mut state, mut handler) = test_state("hello");
        handler.on_key_event(shift_key(KeyCode::Right), &mut state);
        assert_eq!(state.mode, EditorMode::Visual);
        assert!(state.selection.is_some());
    }

    #[test]
    fn select_copy_paste_roundtrip() {
        let (mut state, mut handler) = test_state("hello world");

        // edtui's selection is inclusive on both ends (vim-style), so
        // N shift-rights from col 0 selects N+1 characters, not N --
        // found the hard way when this test first failed with a
        // trailing space included in the copied text.
        for _ in 0..4 {
            handler.on_key_event(shift_key(KeyCode::Right), &mut state); // select "hello"
        }
        handler.on_key_event(ctrl_key('c'), &mut state);
        assert_eq!(state.mode, EditorMode::Insert, "copy should return to typing mode");
        assert!(state.selection.is_none());

        handler.on_key_event(key(KeyCode::End), &mut state);
        handler.on_key_event(ctrl_key('v'), &mut state);

        assert_eq!(String::from(state.lines.clone()), "hello worldhello");
    }

    #[test]
    fn ctrl_x_cuts_the_selection() {
        let (mut state, mut handler) = test_state("hello world");

        for _ in 0..5 {
            handler.on_key_event(shift_key(KeyCode::Right), &mut state); // select "hello " (inclusive selection, see above)
        }
        handler.on_key_event(ctrl_key('x'), &mut state);

        assert_eq!(String::from(state.lines.clone()), "world");
        assert_eq!(state.mode, EditorMode::Insert);

        handler.on_key_event(ctrl_key('v'), &mut state);
        assert_eq!(String::from(state.lines.clone()), "hello world");
    }

    #[test]
    fn esc_cancels_selection_and_returns_to_insert() {
        let (mut state, mut handler) = test_state("hello");
        handler.on_key_event(shift_key(KeyCode::Right), &mut state);
        assert_eq!(state.mode, EditorMode::Visual);

        handler.on_key_event(key(KeyCode::Esc), &mut state);

        assert_eq!(state.mode, EditorMode::Insert);
        assert!(state.selection.is_none());
    }

    /// Reported missing: the command line already had word-wise
    /// `Ctrl+Left`/`Right` (`text_field.rs`), the built-in editor never
    /// did -- unlike a real shell, `edtui`'s custom keymap has no
    /// built-in fallback for an unbound key, so this was a silent
    /// no-op rather than falling back to single-character movement.
    #[test]
    fn ctrl_right_moves_by_a_word_not_one_character() {
        let (mut state, mut handler) = test_state("hello world");
        handler.on_key_event(ctrl_code_key(KeyCode::Right), &mut state);
        assert!(state.cursor.col > 1, "should have moved past just one character: {}", state.cursor.col);
        assert_eq!(state.mode, EditorMode::Insert, "no selection should start");
    }

    #[test]
    fn ctrl_left_moves_back_by_a_word() {
        let (mut state, mut handler) = test_state("hello world");
        state.cursor.col = 11; // end of the line
        handler.on_key_event(ctrl_code_key(KeyCode::Left), &mut state);
        assert!(state.cursor.col < 10, "should have moved back more than one character: {}", state.cursor.col);
    }

    /// A selection started character-wise (`Shift+Right`) should still
    /// extend correctly once switched to word-wise (`Ctrl+Shift+Right`)
    /// mid-selection -- both share the same `state.selection`, so
    /// there's no special handoff needed, but worth pinning down
    /// directly since a user is likely to mix the two in practice (a
    /// few characters, then "grab the rest of this word"). Lives here
    /// rather than in `word_select`'s own test module since it's really
    /// exercising the handoff *between* this table and that function,
    /// not either one in isolation.
    #[test]
    fn switching_from_character_wise_to_word_wise_selection_still_extends() {
        let (mut state, mut handler) = test_state("hello world");
        handler.on_key_event(shift_key(KeyCode::Right), &mut state);
        handler.on_key_event(shift_key(KeyCode::Right), &mut state);
        let after_char_wise = state.selection.as_ref().expect("should have a selection").end;

        extend_word_selection(&mut state, true);
        let after_word_wise = state.selection.as_ref().expect("should still have a selection").end;
        assert!(after_word_wise.col > after_char_wise.col, "word-wise extend should grow past the character-wise selection: {after_char_wise:?} -> {after_word_wise:?}");
    }

    #[test]
    fn ctrl_z_undoes_last_insert() {
        let (mut state, mut handler) = test_state("");
        handler.on_key_event(key(KeyCode::Char('x')), &mut state);
        assert_eq!(String::from(state.lines.clone()), "x");

        handler.on_key_event(ctrl_key('z'), &mut state);
        assert_eq!(String::from(state.lines.clone()), "");
    }
}
