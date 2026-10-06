use crossterm::event::{KeyCode, KeyEvent};
use edtui::{EditorMode, EditorState, Index2};

use super::line_wrap::wrap_line_boundary_arrow_movement;
use super::shift_select::{
    anchor_fresh_shift_home_end, anchor_fresh_shift_selection, close_selection_if_back_on_the_anchors_row, exclude_landing_column_on_fresh_vertical_selection,
    follow_line_break_selection, keep_shift_end_off_the_line_break, VerticalAnchor,
};
use super::is_selection_consuming_key;

/// The `Standard` keymap's correction passes after `edtui`'s own dispatch,
/// in order -- each adds what the declarative table can't express. One
/// function, so `Editor::input` and the binding tests run the same thing:
///
/// 0. `is_selection_consuming_key` -- after `Backspace`/`Delete`/
///    `Ctrl+C/X/V` on a selection, back to plain typing by field
///    assignment (`SwitchMode(Insert)` would add an undo checkpoint).
/// 1. `anchor_fresh_shift_selection` -- a fresh `Shift+Left/Right`
///    selects exactly one character; `anchor_fresh_shift_home_end` keeps
///    a fresh `Shift+Home` off the character under the cursor, and
///    `keep_shift_end_off_the_line_break` `Shift+End` off the line break.
/// 2. `wrap_line_boundary_arrow_movement` -- `Left`/`Right` that didn't
///    move at a line boundary crosses it; skipped when step 1 stopped on
///    a character on purpose.
/// 3. `exclude_landing_column_on_fresh_vertical_selection` -- a fresh
///    `Shift+Up/Down` doesn't select the aligned landing column; the
///    anchor goes to `vertical`. `follow_line_break_selection` keeps a
///    selection from column 0 ending on line breaks.
/// 4. `close_selection_if_back_on_the_anchors_row` -- every
///    `Shift+Up/Down`: back on the anchor's row closes the selection.
///
/// History: docs/history/shift-select.md.
pub(in crate::editor) fn correct_after_dispatch(state: &mut EditorState, key: KeyEvent, cursor_before: Index2, mode_before: EditorMode, vertical: &mut Option<VerticalAnchor>) {
    if mode_before == EditorMode::Visual && is_selection_consuming_key(&key) {
        state.selection = None;
        state.mode = EditorMode::Insert;
    }

    let freshly_entered_visual = mode_before != EditorMode::Visual && state.mode == EditorMode::Visual;
    let is_vertical = matches!(key.code, KeyCode::Up | KeyCode::Down);
    if freshly_entered_visual {
        anchor_fresh_shift_home_end(state, key.code, cursor_before);
        if !is_vertical {
            // A stale anchor from an earlier selection mustn't steer this one.
            *vertical = None;
        }
    }
    keep_shift_end_off_the_line_break(state, key.code);
    let anchored_on_a_real_character = freshly_entered_visual && anchor_fresh_shift_selection(state, key.code, cursor_before);

    if !anchored_on_a_real_character {
        wrap_line_boundary_arrow_movement(state, key.code, key.modifiers, cursor_before);
    }

    if freshly_entered_visual && is_vertical {
        let anchor = VerticalAnchor { position: cursor_before, ends_on_line_break: false };
        *vertical = Some(exclude_landing_column_on_fresh_vertical_selection(state, key.code, anchor));
    } else if mode_before == EditorMode::Visual {
        follow_line_break_selection(state, key.code, cursor_before, vertical);
    }

    close_selection_if_back_on_the_anchors_row(state, key.code, vertical);
}
