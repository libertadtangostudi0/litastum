use std::collections::HashMap;

use crossterm::event::KeyCode;
use edtui::actions::{
    Action, Chainable, CopySelection, DeleteChar, DeleteCharForward, DeleteSelection, LineBreak,
    MoveBackward, MoveDown, MoveForward, MoveHalfPageDown, MoveHalfPageUp, MoveToEndOfLine,
    MoveToStartOfLine, MoveUp, PasteBefore, Redo, SwitchMode, Undo,
};
use edtui::events::{KeyEventHandler, KeyEventRegister, KeyInput};
use edtui::EditorMode;

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

        // Shift+arrow starts (or extends) a selection.
        (i(KeyInput::shift(KeyCode::Left)), SwitchMode(EditorMode::Visual).chain(MoveBackward(1)).into()),
        (i(KeyInput::shift(KeyCode::Right)), SwitchMode(EditorMode::Visual).chain(MoveForward(1)).into()),
        (i(KeyInput::shift(KeyCode::Up)), SwitchMode(EditorMode::Visual).chain(MoveUp(1)).into()),
        (i(KeyInput::shift(KeyCode::Down)), SwitchMode(EditorMode::Visual).chain(MoveDown(1)).into()),
        (v(KeyInput::shift(KeyCode::Left)), MoveBackward(1).into()),
        (v(KeyInput::shift(KeyCode::Right)), MoveForward(1).into()),
        (v(KeyInput::shift(KeyCode::Up)), MoveUp(1).into()),
        (v(KeyInput::shift(KeyCode::Down)), MoveDown(1).into()),

        // Plain movement while a selection is active collapses it.
        (v(KeyInput::new(KeyCode::Left)), exit_selection().chain(MoveBackward(1)).into()),
        (v(KeyInput::new(KeyCode::Right)), exit_selection().chain(MoveForward(1)).into()),
        (v(KeyInput::new(KeyCode::Up)), exit_selection().chain(MoveUp(1)).into()),
        (v(KeyInput::new(KeyCode::Down)), exit_selection().chain(MoveDown(1)).into()),
        (v(KeyInput::new(KeyCode::Home)), exit_selection().chain(MoveToStartOfLine()).into()),
        (v(KeyInput::new(KeyCode::End)), exit_selection().chain(MoveToEndOfLine()).into()),
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
    use crate::test_support::{ctrl_key, key, shift_key};

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

    #[test]
    fn ctrl_z_undoes_last_insert() {
        let (mut state, mut handler) = test_state("");
        handler.on_key_event(key(KeyCode::Char('x')), &mut state);
        assert_eq!(String::from(state.lines.clone()), "x");

        handler.on_key_event(ctrl_key('z'), &mut state);
        assert_eq!(String::from(state.lines.clone()), "");
    }
}
