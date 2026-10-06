use crossterm::event::KeyCode;
use edtui::actions::{Execute, MoveBackward, MoveForward, SwitchMode};
use edtui::{EditorMode, EditorState, Index2};

/// Where a `Shift+Up`/`Down` selection started (`Editor::vertical_shift_anchor`),
/// before any trim, and whether its live end sits on a line break.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::editor) struct VerticalAnchor {
    pub position: Index2,
    /// Set while a selection from column 0 runs downward: `edtui` has no
    /// column before 0 to trim to, so the end (and cursor) sits at
    /// `(row, len)` -- that row's line break, which `edtui` counts as
    /// selected -- one row above where the caret is meant to be.
    pub ends_on_line_break: bool,
}

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
/// that row. Column 0 has nothing to trim to, and used to select the far
/// row's first character: the bound moves to the line break of the row
/// before it instead (`row_len` -- `edtui` counts `(row, len)` as that
/// line break), so whole lines are selected, as in VS Code.
pub(in crate::editor) fn exclude_landing_column_on_fresh_vertical_selection(state: &mut EditorState, key_code: KeyCode, mut anchor: VerticalAnchor) -> VerticalAnchor {
    let moved = state.cursor.row != anchor.position.row;
    match key_code {
        KeyCode::Down if state.cursor.col == 0 => {
            if moved {
                let row = state.cursor.row - 1;
                set_selection_end(state, Index2::new(row, row_len(state, row)));
                anchor.ends_on_line_break = true;
            }
        }
        KeyCode::Down => {
            state.cursor.col -= 1;
            if let Some(selection) = state.selection.as_mut() {
                selection.end = state.cursor;
            }
        }
        KeyCode::Up => {
            let line_break_above = anchor.position.row.checked_sub(1).map(|row| Index2::new(row, row_len(state, row)));
            if let Some(selection) = state.selection.as_mut() {
                if selection.start.col > 0 {
                    selection.start.col -= 1;
                } else if let (true, Some(bound)) = (moved, line_break_above) {
                    selection.start = bound;
                }
            }
        }
        _ => {}
    }
    anchor
}

/// `Shift+Up`/`Down` while the selection ends on a line break
/// (`VerticalAnchor::ends_on_line_break`): `edtui`'s own move would leave
/// the break for a column, so the end goes to the next or previous row's
/// break instead. Back up to the anchor's row, the cursor returns to the
/// anchor for `close_selection_if_back_on_the_anchors_row`. A selection
/// changed some other way (`Shift+Left`) no longer ends on a break and
/// goes back to the plain behavior.
pub(in crate::editor) fn follow_line_break_selection(state: &mut EditorState, key_code: KeyCode, cursor_before: Index2, vertical: &mut Option<VerticalAnchor>) {
    let Some(anchor) = vertical.as_mut() else {
        return;
    };
    if !anchor.ends_on_line_break || !matches!(key_code, KeyCode::Up | KeyCode::Down) {
        return;
    }
    if state.selection.is_none() || cursor_before.col != row_len(state, cursor_before.row) {
        anchor.ends_on_line_break = false;
        return;
    }
    match key_code {
        KeyCode::Down => {
            let next = cursor_before.row + 1;
            let end = if next < state.lines.len() { Index2::new(next, row_len(state, next)) } else { cursor_before };
            set_selection_end(state, end);
        }
        _ if cursor_before.row == anchor.position.row => {
            anchor.ends_on_line_break = false;
            state.cursor = anchor.position;
        }
        _ => {
            let row = cursor_before.row - 1;
            set_selection_end(state, Index2::new(row, row_len(state, row)));
        }
    }
}

/// Runs on every `Shift+Up`/`Down` press: once the cursor is back on the
/// anchor's row, closes the selection (`edtui` can't represent an empty
/// one) and restores the anchor's untrimmed column, so the fresh-press
/// trim above doesn't leave the cursor one column short after a round
/// trip. Without a stored anchor (a selection started sideways), the
/// selection's own start row stands in.
pub(in crate::editor) fn close_selection_if_back_on_the_anchors_row(state: &mut EditorState, key_code: KeyCode, vertical: &mut Option<VerticalAnchor>) {
    if !matches!(key_code, KeyCode::Up | KeyCode::Down) {
        return;
    }
    let Some(selection) = state.selection.as_ref() else {
        return;
    };
    if vertical.is_some_and(|anchor| anchor.ends_on_line_break) {
        return;
    }
    let anchor_row = vertical.map_or(selection.start.row, |anchor| anchor.position.row);
    if state.cursor.row != anchor_row {
        return;
    }
    if let Some(anchor) = vertical.take() {
        state.cursor.col = anchor.position.col;
    }
    SwitchMode(EditorMode::Normal).execute(state);
    SwitchMode(EditorMode::Insert).execute(state);
}

/// A fresh `Shift+Home`/`Shift+End`: `SwitchMode(Visual)` anchored the
/// cell under the cursor, which `Home` must not take (the caret sits
/// before it) -- the anchor moves one cell left. Nothing to select
/// (`Home` at column 0, `End` at the line's end) closes the selection.
pub(in crate::editor) fn anchor_fresh_shift_home_end(state: &mut EditorState, key_code: KeyCode, cursor_before: Index2) {
    let len = row_len(state, cursor_before.row);
    match key_code {
        KeyCode::Home if cursor_before.col == 0 => close_at(state, cursor_before),
        KeyCode::End if cursor_before.col >= len => close_at(state, cursor_before),
        KeyCode::Home => {
            if let Some(selection) = state.selection.as_mut() {
                selection.start = Index2::new(cursor_before.row, cursor_before.col.min(len) - 1);
            }
        }
        _ => {}
    }
}

/// `Shift+End`: `MoveToEndOfLine` in `Visual` lands on `(row, len)`, the
/// line break, which `edtui` would select too -- the end goes back onto
/// the last character.
pub(in crate::editor) fn keep_shift_end_off_the_line_break(state: &mut EditorState, key_code: KeyCode) {
    if key_code != KeyCode::End || state.mode != EditorMode::Visual {
        return;
    }
    let row = state.cursor.row;
    let len = row_len(state, row);
    if len > 0 && state.cursor.col >= len {
        set_selection_end(state, Index2::new(row, len - 1));
    }
}

fn close_at(state: &mut EditorState, cursor: Index2) {
    SwitchMode(EditorMode::Normal).execute(state);
    SwitchMode(EditorMode::Insert).execute(state);
    state.cursor = cursor;
}

fn set_selection_end(state: &mut EditorState, end: Index2) {
    state.cursor = end;
    if let Some(selection) = state.selection.as_mut() {
        selection.end = end;
    }
}

fn row_len(state: &EditorState, row: usize) -> usize {
    state.lines.len_col(row).unwrap_or(0)
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
        anchor_fresh_shift_selection, close_selection_if_back_on_the_anchors_row, exclude_landing_column_on_fresh_vertical_selection, VerticalAnchor,
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
        let anchor = VerticalAnchor { position: state.cursor, ends_on_line_break: false };
        let mut true_anchor_col = Some(exclude_landing_column_on_fresh_vertical_selection(&mut state, crossterm::event::KeyCode::Down, anchor));
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

        exclude_landing_column_on_fresh_vertical_selection(&mut state, crossterm::event::KeyCode::Down, anchor_at(0, 4));

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

        exclude_landing_column_on_fresh_vertical_selection(&mut state, crossterm::event::KeyCode::Up, anchor_at(1, 4));

        assert_eq!(state.selection.unwrap().start.col, 3);
        assert_eq!(state.cursor.col, 4, "MoveUp never touches .col -- this function must not either, for Up");
    }

    fn anchor_at(row: usize, col: usize) -> VerticalAnchor {
        VerticalAnchor { position: edtui::Index2::new(row, col), ends_on_line_break: false }
    }

    /// Column 0 has nothing to trim into: the end moves to the line break
    /// of the row above (`(0, 12)`, past "terminal one") instead of taking
    /// the landing row's first character, as it used to.
    #[test]
    fn down_from_column_zero_ends_on_the_line_break_above() {
        let mut state = state_for("terminal one\nterminal two", 0);
        state.cursor.row = 1;
        if let Some(selection) = state.selection.as_mut() {
            selection.end.row = 1;
        }

        let anchor = exclude_landing_column_on_fresh_vertical_selection(&mut state, crossterm::event::KeyCode::Down, anchor_at(0, 0));

        assert_eq!(state.cursor, edtui::Index2::new(0, 12));
        assert_eq!(state.selection.unwrap().end, state.cursor, "selection.end must stay in lock-step with the cursor");
        assert!(anchor.ends_on_line_break);
    }
}
