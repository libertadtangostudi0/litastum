use crossterm::event::{KeyCode, KeyModifiers};
use edtui::actions::{Execute, MoveDown, MoveToEndOfLine, MoveToStartOfLine, MoveUp};
use edtui::{EditorState, Index2};

/// `Left` at a line's start / `Right` at its end moves to the adjacent
/// line. Runs after the unmodified table, and only if the cursor didn't
/// move: `edtui`'s moves are column-only, and a table chain can't be
/// conditional. Uses `edtui`'s own line motions, which extend a
/// `Visual` selection too, so `Shift+Left`/`Right` wrap as well. Not for
/// `Ctrl` (word-wise). History: docs/history/editor-keymap.md.
pub(in crate::editor) fn wrap_line_boundary_arrow_movement(state: &mut EditorState, key_code: KeyCode, modifiers: KeyModifiers, cursor_before: Index2) {
    if modifiers.contains(KeyModifiers::CONTROL) {
        return;
    }
    if state.cursor != cursor_before {
        // The table's own handling already moved the cursor -- nothing
        // to add, and this covers every key besides plain/shifted
        // `Left`/`Right` too (they never move `state.cursor` here at
        // all, since this function is only ever called for those two
        // key codes -- see `Editor::input`'s call site).
        return;
    }

    match key_code {
        KeyCode::Left if cursor_before.row > 0 => {
            MoveUp(1).execute(state);
            MoveToEndOfLine().execute(state);
        }
        KeyCode::Right if cursor_before.row < state.lines.len().saturating_sub(1) => {
            MoveDown(1).execute(state);
            MoveToStartOfLine().execute(state);
        }
        _ => {}
    }
}


#[cfg(test)]
mod tests {
    use edtui::clipboard::InternalClipboard;
    use edtui::{EditorEventHandler, EditorMode, EditorState, Lines};

    use super::{super::{shift_select::anchor_fresh_shift_selection, standard_key_handler}, wrap_line_boundary_arrow_movement};
    use crate::test_support::{key, shift_key};

    /// Same shape as `bindings::tests::test_state` -- a raw
    /// `EditorState` + our real keymap, `InternalClipboard` so nothing
    /// touches the real system clipboard.
    fn test_state(contents: &str) -> (EditorState, EditorEventHandler) {
        let mut state = EditorState::new(Lines::from(contents));
        state.mode = EditorMode::Insert;
        state.set_clipboard(InternalClipboard::default());
        (state, EditorEventHandler::new(standard_key_handler(true)))
    }

    /// Drives one key through the real table, then the same two
    /// correction passes in the same order `Editor::input` uses --
    /// without needing a real file on disk (`Editor::open` requires
    /// one). See `Editor::input`'s own doc comment for why the wrap
    /// check is skipped when a fresh shift-selection just anchored on a
    /// real character.
    fn input(state: &mut EditorState, handler: &mut EditorEventHandler, key_event: crossterm::event::KeyEvent) {
        let cursor_before = state.cursor;
        let mode_before = state.mode;
        handler.on_key_event(key_event, state);

        let freshly_entered_visual = mode_before != EditorMode::Visual && state.mode == EditorMode::Visual;
        let anchored_on_a_real_character = freshly_entered_visual && anchor_fresh_shift_selection(state, key_event.code, cursor_before);

        if !anchored_on_a_real_character {
            wrap_line_boundary_arrow_movement(state, key_event.code, key_event.modifiers, cursor_before);
        }
    }

    #[test]
    fn left_at_start_of_line_moves_to_the_end_of_the_previous_line() {
        let (mut state, mut handler) = test_state("hello\nworld");
        state.cursor.row = 1;
        state.cursor.col = 0;

        input(&mut state, &mut handler, key(crossterm::event::KeyCode::Left));

        assert_eq!(state.cursor.row, 0, "should have moved up onto the previous line");
        assert_eq!(state.cursor.col, 5, "should land right after \"hello\"'s own last letter, the append position");
    }

    #[test]
    fn right_at_end_of_line_moves_to_the_start_of_the_next_line() {
        let (mut state, mut handler) = test_state("hello\nworld");
        state.cursor.row = 0;
        state.cursor.col = 5; // right after "hello", nothing left to move forward into on this line

        input(&mut state, &mut handler, key(crossterm::event::KeyCode::Right));

        assert_eq!(state.cursor.row, 1, "should have moved down onto the next line");
        assert_eq!(state.cursor.col, 0, "should land at the very start of \"world\"");
    }

    /// The whole point of intercepting *after* the table runs, not
    /// instead of it: ordinary mid-line movement must be completely
    /// unaffected -- this is the same regression class as every other
    /// "fix without breaking existing behavior" test in this module.
    #[test]
    fn ordinary_mid_line_movement_is_unaffected() {
        let (mut state, mut handler) = test_state("hello\nworld");
        state.cursor.row = 0;
        state.cursor.col = 2;

        input(&mut state, &mut handler, key(crossterm::event::KeyCode::Right));
        assert_eq!((state.cursor.row, state.cursor.col), (0, 3), "plain mid-line Right should move exactly one column, nothing else");

        input(&mut state, &mut handler, key(crossterm::event::KeyCode::Left));
        assert_eq!((state.cursor.row, state.cursor.col), (0, 2), "plain mid-line Left should move exactly one column back, nothing else");
    }

    /// `Left` on the very first line's own column 0 (nothing to wrap
    /// to) must stay put, not panic or wrap around to the last line.
    #[test]
    fn left_at_the_very_start_of_the_buffer_does_not_wrap() {
        let (mut state, mut handler) = test_state("hello\nworld");
        input(&mut state, &mut handler, key(crossterm::event::KeyCode::Left));
        assert_eq!((state.cursor.row, state.cursor.col), (0, 0));
    }

    /// `Right` on the very last line's own end (nothing to wrap to)
    /// must stay put too.
    #[test]
    fn right_at_the_very_end_of_the_buffer_does_not_wrap() {
        let (mut state, mut handler) = test_state("hello\nworld");
        state.cursor.row = 1;
        state.cursor.col = 5; // right after "world"
        input(&mut state, &mut handler, key(crossterm::event::KeyCode::Right));
        assert_eq!((state.cursor.row, state.cursor.col), (1, 5));
    }

    /// `Ctrl` makes the correction a no-op. Called directly, not through
    /// the table: `edtui`'s `MoveWordBackward` already crosses lines on
    /// its own.
    #[test]
    fn control_modifier_prevents_the_wrap_check_from_acting() {
        let (mut state, _handler) = test_state("hello\nworld");
        state.cursor.row = 1;
        state.cursor.col = 0;
        let cursor_before = state.cursor;

        wrap_line_boundary_arrow_movement(&mut state, crossterm::event::KeyCode::Left, crossterm::event::KeyModifiers::CONTROL, cursor_before);

        assert_eq!(state.cursor.row, 1, "Ctrl held should make this a no-op regardless of the key code or cursor position");
        assert_eq!(state.cursor.col, 0);
    }

    /// A `Shift+Right` selection, already extended to the end of a
    /// line, must keep extending across the line boundary rather than
    /// stalling -- and the selection itself (not just the cursor) has
    /// to reflect it, confirming `MoveDown`/`MoveToStartOfLine`'s own
    /// `Visual`-mode selection update (verified directly from `edtui`'s
    /// source) really does fire from here.
    #[test]
    fn shift_right_selection_extends_across_the_line_boundary() {
        let (mut state, mut handler) = test_state("hi\nworld");
        state.cursor.row = 0;
        state.cursor.col = 2; // right after "hi", nothing left on this line

        input(&mut state, &mut handler, shift_key(crossterm::event::KeyCode::Right));

        assert_eq!(state.mode, EditorMode::Visual, "should have started a selection, same as any other Shift+Right");
        let selection = state.selection.expect("should have started a selection");
        assert_eq!(state.cursor, edtui::Index2 { row: 1, col: 0 }, "cursor should have wrapped onto the start of the next line");
        assert_eq!(selection.end, state.cursor, "the selection's own end must always equal the cursor, same invariant as everywhere else");
    }
}
