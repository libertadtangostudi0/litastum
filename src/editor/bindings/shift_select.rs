use crossterm::event::KeyCode;
use edtui::actions::{Execute, MoveBackward, MoveForward, SwitchMode};
use edtui::{EditorMode, EditorState, Index2};

/// Resolves the first press of a fresh `Shift+Left`/`Right` selection so
/// N presses select exactly N characters. `SwitchMode(Visual)` already
/// anchors a one-cell selection on the current cell (vim's `v`), so the
/// fresh table entries don't chain a `Move` -- this decides the one cell:
/// `Right` keeps the cell the cursor is on, `Left` takes the cell it
/// moves onto. `Up`/`Down` pass through untouched (`_ => true`).
///
/// Returns `true` when the press is fully resolved, so `Editor::input`
/// skips the line-boundary wrap check; `false` when a real boundary may
/// have been hit (`Right` fell back to a move, `Left` made no progress).
///
/// History: docs/history/shift-select.md.
pub(in crate::editor) fn anchor_fresh_shift_selection(state: &mut EditorState, key_code: KeyCode, cursor_before: Index2) -> bool {
    match key_code {
        KeyCode::Right => forward_anchor(state, cursor_before, MoveForward(1)),
        KeyCode::Left => backward_anchor(state, cursor_before, MoveBackward(1)),
        _ => true,
    }
}

/// Keeps a fresh `Shift+Up`/`Down` from selecting the landing column on
/// the far row (`MoveUp`/`MoveDown` never change `.col`, and the
/// selection is inclusive). Runs once, on the press that opens the
/// selection; the trimmed column then carries forward on its own.
///
/// `Down` trims the destination (`state.cursor`, kept equal to
/// `selection.end`); `Up` trims `selection.start`, the bottom edge --
/// whichever raw field sits on the larger row is an inclusive bound on
/// that row. Column 0 is left alone.
pub(in crate::editor) fn exclude_landing_column_on_fresh_vertical_selection(state: &mut EditorState, key_code: KeyCode) {
    match key_code {
        KeyCode::Down => {
            if state.cursor.col == 0 {
                return;
            }
            state.cursor.col -= 1;
            if let Some(selection) = state.selection.as_mut() {
                selection.end = state.cursor;
            }
        }
        KeyCode::Up => {
            if let Some(selection) = state.selection.as_mut() {
                if selection.start.col > 0 {
                    selection.start.col -= 1;
                }
            }
        }
        _ => {}
    }
}

/// Runs on every `Shift+Up`/`Down` press: once the cursor is back on the
/// anchor's row, closes the selection (`edtui` can't represent an empty
/// one) and restores the untrimmed column from `true_anchor_col`
/// (`Editor::vertical_shift_anchor_col`), so the fresh-press trim above
/// doesn't leave the cursor one column short after a round trip.
pub(in crate::editor) fn close_selection_if_back_on_the_anchors_row(
    state: &mut EditorState,
    key_code: KeyCode,
    true_anchor_col: &mut Option<usize>,
) {
    if !matches!(key_code, KeyCode::Up | KeyCode::Down) {
        return;
    }
    let Some(selection) = state.selection.as_ref() else {
        return;
    };
    if state.cursor.row != selection.start.row {
        return;
    }
    if let Some(col) = true_anchor_col.take() {
        state.cursor.col = col;
    }
    SwitchMode(EditorMode::Normal).execute(state);
    SwitchMode(EditorMode::Insert).execute(state);
}

/// `Right`: the anchor cell (where the cursor already sits) is exactly
/// the first character a forward selection should include -- stop
/// there if it's real, otherwise fall back to actually performing the
/// move (nothing valid to anchor on, e.g. the append position past a
/// line's last character).
fn forward_anchor(state: &mut EditorState, cursor_before: Index2, mut move_action: impl Execute) -> bool {
    if state.lines.get(cursor_before).is_some() {
        return true;
    }
    move_action.execute(state);
    false
}

/// `Left`: the cell to select is the one the cursor moves onto. Moves
/// first; on real progress the anchor follows, leaving exactly that one
/// cell selected. No progress leaves the anchor and returns `false`, so
/// the line-boundary wrap can cross to the previous line.
fn backward_anchor(state: &mut EditorState, cursor_before: Index2, mut move_action: impl Execute) -> bool {
    move_action.execute(state);
    if state.cursor == cursor_before {
        return false;
    }
    if let Some(selection) = state.selection.as_mut() {
        selection.start = state.cursor;
    }
    true
}


#[cfg(test)]
mod tests {
    use edtui::actions::{Execute, SwitchMode};
    use edtui::{EditorMode, EditorState, Lines};

    use super::{
        anchor_fresh_shift_selection, close_selection_if_back_on_the_anchors_row, exclude_landing_column_on_fresh_vertical_selection,
    };

    /// Uses the real `SwitchMode(Visual)` action: `Selection` is
    /// `pub(crate)`, so this is the only way to get a real one. `mode`
    /// is set to `Insert` before `cursor.col` because `SwitchMode` clamps
    /// with the current mode first, and the default mode clamps the
    /// append position (`col == len`) back onto a real character.
    fn state_for(contents: &str, cursor_col: usize) -> EditorState {
        let mut state = EditorState::new(Lines::from(contents));
        state.mode = EditorMode::Insert;
        state.cursor.col = cursor_col;
        SwitchMode(EditorMode::Visual).execute(&mut state);
        state
    }

    #[test]
    fn right_stops_immediately_on_a_real_character() {
        let mut state = state_for("Draft", 0);
        let cursor_before = state.cursor;
        let handled = anchor_fresh_shift_selection(&mut state, crossterm::event::KeyCode::Right, cursor_before);
        assert!(handled);
        assert_eq!(state.cursor.col, 0, "should not have moved at all -- the anchor cell alone is the whole selection");
    }

    #[test]
    fn right_falls_back_to_a_real_move_at_the_append_position() {
        let mut state = state_for("hi", 2); // one past 'i', nothing real there
        let cursor_before = state.cursor;
        let handled = anchor_fresh_shift_selection(&mut state, crossterm::event::KeyCode::Right, cursor_before);
        assert!(!handled, "nothing real to anchor on -- should have fallen back to an actual move");
    }

    /// Regression: `Shift+Left` selected the character to the *right*.
    #[test]
    fn left_selects_the_character_actually_to_the_left() {
        let mut state = state_for("Draft", 2); // cursor on 'a', the third letter
        let cursor_before = state.cursor;
        let handled = anchor_fresh_shift_selection(&mut state, crossterm::event::KeyCode::Left, cursor_before);

        assert!(handled, "real progress was made -- this press is fully resolved, no wrap check needed");
        assert_eq!(state.cursor.col, 1, "should have moved onto 'r', the character actually to the left");
        let selection = state.selection.expect("should still have a selection");
        assert_eq!(selection.start, state.cursor, "the anchor should have been dragged to match -- exactly one cell selected");
        assert_eq!(selection.end, state.cursor);
    }

    /// `Shift+Left` with nothing before the cursor at all (column 0)
    /// must not fabricate a selection out of thin air -- reports "not
    /// yet handled" so the line-boundary wrap check (or, at the very
    /// start of the whole buffer, simply nothing) can decide instead.
    #[test]
    fn left_at_the_very_start_of_a_line_reports_unhandled() {
        let mut state = state_for("Draft", 0);
        let cursor_before = state.cursor;
        let handled = anchor_fresh_shift_selection(&mut state, crossterm::event::KeyCode::Left, cursor_before);
        assert!(!handled, "no progress was possible -- must not claim this press is resolved");
        assert_eq!(state.cursor.col, 0, "should not have moved");
    }

    /// Simulates a vertical excursion landing back on the anchor's row;
    /// full round trips through real keys are in `bindings/tests.rs`.
    #[test]
    fn closes_the_selection_once_the_cursor_is_back_on_the_anchors_row() {
        let mut state = state_for("Draft", 1);
        let anchor_row = state.selection.as_ref().expect("SwitchMode(Visual) should anchor a selection").start.row;
        state.cursor.row = anchor_row;

        close_selection_if_back_on_the_anchors_row(&mut state, crossterm::event::KeyCode::Down, &mut None);

        assert!(state.selection.is_none(), "should have collapsed the selection entirely");
        assert_eq!(state.mode, EditorMode::Insert);
    }

    #[test]
    fn leaves_the_selection_alone_while_still_on_a_different_row() {
        let mut state = state_for("Draft", 1);
        state.cursor.row = 5; // nowhere near the anchor's own row

        close_selection_if_back_on_the_anchors_row(&mut state, crossterm::event::KeyCode::Down, &mut None);

        assert!(state.selection.is_some(), "should still be selecting -- the excursion isn't over yet");
    }

    #[test]
    fn is_a_no_op_for_keys_other_than_up_or_down() {
        let mut state = state_for("Draft", 1);
        let anchor_row = state.selection.as_ref().expect("SwitchMode(Visual) should anchor a selection").start.row;
        state.cursor.row = anchor_row;

        close_selection_if_back_on_the_anchors_row(&mut state, crossterm::event::KeyCode::Right, &mut None);

        assert!(state.selection.is_some(), "Right/Left have their own handling -- this function must not touch them");
    }

    /// Regression: without restoring the tracked column, a round trip
    /// left the cursor one column short.
    #[test]
    fn restores_the_true_anchor_column_once_closed() {
        let mut state = state_for("terminal one\nterminal two", 4); // between 't' and 'e'
        let mut true_anchor_col = Some(state.cursor.col);
        exclude_landing_column_on_fresh_vertical_selection(&mut state, crossterm::event::KeyCode::Down);
        assert_eq!(state.cursor.col, 3, "should have trimmed the landing column by one");

        let anchor_row = state.selection.as_ref().unwrap().start.row;
        state.cursor.row = anchor_row; // simulate the reversing Up press landing back here

        close_selection_if_back_on_the_anchors_row(&mut state, crossterm::event::KeyCode::Up, &mut true_anchor_col);

        assert_eq!(state.cursor.col, 4, "should be back on the exact column the excursion started from");
        assert!(true_anchor_col.is_none(), "should have consumed the tracked value");
    }

    /// Regression (aligned text): the destination row's copy of the
    /// word on the same column must not be selected.
    #[test]
    fn down_trims_the_destination_rows_landing_column() {
        let mut state = state_for("terminal one\nterminal two", 4);
        state.cursor.row = 1; // as if MoveDown(1) already ran
        if let Some(selection) = state.selection.as_mut() {
            selection.end.row = 1;
        }

        exclude_landing_column_on_fresh_vertical_selection(&mut state, crossterm::event::KeyCode::Down);

        assert_eq!(state.cursor.col, 3);
        assert_eq!(state.selection.unwrap().end.col, 3, "selection.end must stay in lock-step with the cursor");
    }

    /// `Up`: the trim applies to `selection.start` instead (the row the
    /// press started on, now the selection's own bottom edge) --
    /// `state.cursor` itself is untouched, since `MoveUp` lands it on a
    /// different row than the one being trimmed.
    #[test]
    fn up_trims_the_anchors_own_starting_column() {
        let mut state = state_for("terminal one\nterminal two", 4);
        state.cursor.row = 1; // press started on row 1
        if let Some(selection) = state.selection.as_mut() {
            selection.start.row = 1;
        }
        state.cursor.row = 0; // as if MoveUp(1) already ran

        exclude_landing_column_on_fresh_vertical_selection(&mut state, crossterm::event::KeyCode::Up);

        assert_eq!(state.selection.unwrap().start.col, 3);
        assert_eq!(state.cursor.col, 4, "MoveUp never touches .col -- this function must not either, for Up");
    }

    /// Column 0 has nothing to trim into -- must not underflow.
    #[test]
    fn makes_no_adjustment_at_column_zero() {
        let mut state = state_for("terminal one\nterminal two", 0);
        state.cursor.row = 1;
        if let Some(selection) = state.selection.as_mut() {
            selection.end.row = 1;
        }

        exclude_landing_column_on_fresh_vertical_selection(&mut state, crossterm::event::KeyCode::Down);

        assert_eq!(state.cursor.col, 0);
    }
}
