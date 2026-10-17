use super::*;
use crate::test_support::unique_scratch_dir;

fn open_pair(left_content: &str, right_content: &str) -> CompareState {
    let dir = unique_scratch_dir("compare-state");
    let left_path = dir.join("left.txt");
    let right_path = dir.join("right.txt");
    std::fs::write(&left_path, left_content).unwrap();
    std::fs::write(&right_path, right_content).unwrap();
    CompareState::open(left_path, right_path, None, EditorKeymapMode::Standard).unwrap()
}

/// Regression coverage: an earlier version detected line endings
/// from `Editor::text()` at render time, which can never see a
/// `CRLF` at all -- `edtui::Lines::from` normalizes `\r\n` to `\n`
/// on load (`str::lines()`, confirmed directly, strips both
/// uniformly), so `Editor::text()` never contains a `\r` in the
/// first place. `line_endings` has to come from a real, separate
/// read of the file's own raw bytes, taken once at `open`.
#[test]
fn line_endings_reflect_the_files_own_real_crlf_even_though_the_live_buffer_never_sees_it() {
    let state = open_pair("a\r\nb\n", "a\nb\n");
    assert_eq!(state.left.text(), "a\nb\n", "the live edtui buffer really does lose the \\r on load");
    assert_eq!(state.line_endings(Side::Left), &[Some(LineEnding::Crlf), Some(LineEnding::Lf)]);
    assert_eq!(state.line_endings(Side::Right), &[Some(LineEnding::Lf), Some(LineEnding::Lf)]);
}

#[test]
fn open_starts_focused_on_the_left_pane() {
    let state = open_pair("a\nb\nc\n", "a\nx\nc\n");
    assert_eq!(state.focus, Side::Left);
    assert_eq!(state.focused().text(), "a\nb\nc\n", "Editor::text() should round-trip the file's own content exactly");
}

#[test]
fn toggle_focus_switches_panes_and_preserves_each_sides_own_cursor() {
    let mut state = open_pair("a\nb\nc\n", "x\ny\nz\n");
    state.left.set_cursor(Index2::new(2, 0));
    state.toggle_focus();
    assert_eq!(state.focus, Side::Right);
    assert_eq!(state.right.cursor(), Index2::new(0, 0), "right hasn't been touched yet, still at its own default");

    state.right.set_cursor(Index2::new(1, 0));
    state.toggle_focus();
    assert_eq!(state.focus, Side::Left);
    assert_eq!(state.left.cursor(), Index2::new(2, 0), "left's own cursor should have survived the round trip");

    state.toggle_focus();
    assert_eq!(state.right.cursor(), Index2::new(1, 0), "right's own cursor should also have survived");
}

#[test]
fn a_submitted_path_loads_into_the_focused_pane() {
    let mut state = open_pair("a\n", "b\n");
    let other = unique_scratch_dir("compare-path-edit").join("other.txt");
    std::fs::write(&other, "x\r\ny\n").unwrap();
    state.toggle_focus();

    state.start_path_edit();
    state.path_edit.as_mut().unwrap().field.set_text(other.to_string_lossy().into_owned());
    state.path_edit_key(crate::test_support::key(crossterm::event::KeyCode::Enter)).unwrap();

    assert!(state.path_edit.is_none());
    assert_eq!(state.right.path(), other);
    assert_eq!(state.right.text(), "x\ny\n");
    assert_eq!(state.line_endings(Side::Right), &[Some(LineEnding::Crlf), Some(LineEnding::Lf)]);
    assert_eq!(state.left.text(), "a\n", "the other pane stays");
}

#[test]
fn a_path_that_cant_be_loaded_keeps_the_field_open() {
    let mut state = open_pair("a\n", "b\n");
    state.start_path_edit();
    state.path_edit.as_mut().unwrap().field.set_text("no/such/file.txt");

    assert!(state.path_edit_key(crate::test_support::key(crossterm::event::KeyCode::Enter)).is_err());
    assert!(state.path_edit.is_some());
    assert_eq!(state.left.text(), "a\n");
}

#[test]
fn a_dirty_pane_isnt_replaced() {
    let mut state = open_pair("a\n", "b\n");
    state.left.input(crate::test_support::key(crossterm::event::KeyCode::Char('!')));
    let other = unique_scratch_dir("compare-path-edit").join("other.txt");
    std::fs::write(&other, "x\n").unwrap();

    assert!(state.replace_focused(other).is_err());
    assert!(state.left.text().starts_with('!'));
}

#[test]
fn is_dirty_reflects_either_pane() {
    let mut state = open_pair("a\n", "b\n");
    assert!(!state.is_dirty());
    state.left.input(crate::test_support::key(crossterm::event::KeyCode::Char('!')));
    assert!(state.is_dirty());
}

#[test]
fn jump_to_next_hunk_moves_the_focused_panes_cursor() {
    let mut state = open_pair("a\nb\nc\nd\n", "a\nx\nc\ny\n");
    state.jump_to_next_hunk();
    assert_eq!(state.left.cursor().row, 1);
    state.jump_to_next_hunk();
    assert_eq!(state.left.cursor().row, 3);
    state.jump_to_next_hunk();
    assert_eq!(state.left.cursor().row, 3, "no further hunk past the last one -- no-op");
}

#[test]
fn jump_to_previous_hunk_moves_the_focused_panes_cursor_backward() {
    let mut state = open_pair("a\nb\nc\nd\n", "a\nx\nc\ny\n");
    state.left.set_cursor(Index2::new(3, 0));
    state.jump_to_previous_hunk();
    assert_eq!(state.left.cursor().row, 1);
    state.jump_to_previous_hunk();
    assert_eq!(state.left.cursor().row, 1, "no earlier hunk before the first one -- no-op");
}

/// Regression coverage for the real report: a multi-line hunk used
/// to stop `F8`/`jump_to_next_hunk` on every single changed line
/// inside it before finally moving to the next real hunk -- here,
/// rows 1-2 on the left are one hunk (both changed), row 4 is a
/// second, separate one-line hunk. A single press from row 0 should
/// skip straight past the *entire* first hunk to the second one,
/// not stop at row 2 first.
#[test]
fn jump_to_next_hunk_skips_the_whole_current_hunk_not_one_line_at_a_time() {
    let mut state = open_pair("a\nb\nc\nd\ne\n", "a\nx\ny\nd\nz\n");

    state.jump_to_next_hunk();
    assert_eq!(state.left.cursor().row, 1, "should land on the first row of the first (two-line) hunk");

    state.jump_to_next_hunk();
    assert_eq!(state.left.cursor().row, 4, "should skip straight past row 2 (still the same hunk) to the second hunk");
}

/// The other direction of the same fix: retreating from inside a
/// multi-line hunk should land on that *previous* hunk's own first
/// row in one press, not step back through it one line at a time.
#[test]
fn jump_to_previous_hunk_skips_the_whole_current_hunk_not_one_line_at_a_time() {
    let mut state = open_pair("a\nb\nc\nd\ne\n", "a\nx\ny\nd\nz\n");
    state.left.set_cursor(Index2::new(4, 0));

    state.jump_to_previous_hunk();
    assert_eq!(state.left.cursor().row, 1, "should land on the first row of the earlier two-line hunk, not row 2 (its last row)");
}

/// Pressing "previous" while sitting *inside* the first hunk in the
/// file (not just on its first row) has nowhere earlier to go --
/// same "no-op past the boundary" contract
/// `jump_to_previous_hunk_moves_the_focused_panes_cursor_backward`
/// already covers for landing exactly on a hunk's own start, just
/// confirmed here from a row mid-hunk too.
#[test]
fn jump_to_previous_hunk_from_mid_first_hunk_is_a_noop() {
    let mut state = open_pair("a\nb\nc\nd\ne\n", "a\nx\ny\nd\nz\n");
    state.left.set_cursor(Index2::new(2, 0)); // second row of the first (rows 1-2) hunk

    state.jump_to_previous_hunk();
    assert_eq!(state.left.cursor().row, 2, "no hunk earlier than the file's own first one -- the cursor shouldn't move at all");
}

/// Lines inserted on the right add padding rows to the left's diff, so
/// diff rows and real rows drift apart. The jump lands on the real row
/// of the hunk ("d", row 3), not on its diff row (5) -- the cursor
/// used to end up below the highlighted hunk.
#[test]
fn hunk_jumps_land_on_the_real_row_after_padding_rows() {
    let mut state = open_pair("a\nb\nc\nd\ne\n", "a\nnew1\nnew2\nb\nc\nX\ne\n");

    state.jump_to_next_hunk();
    assert_eq!(state.left.cursor().row, 3, "\"d\", the replaced line");

    state.jump_to_next_hunk();
    assert_eq!(state.left.cursor().row, 3, "no hunk after it on the left -- the insertion is only padding here");

    state.toggle_focus();
    state.jump_to_next_hunk();
    assert_eq!(state.right.cursor().row, 1, "the insertion on the right");
    state.jump_to_next_hunk();
    assert_eq!(state.right.cursor().row, 5, "\"X\"");
    state.jump_to_previous_hunk();
    assert_eq!(state.right.cursor().row, 1);
}

#[test]
fn jump_to_next_hunk_tracks_whichever_pane_is_focused() {
    let mut state = open_pair("a\nb\nc\nd\n", "a\nx\nc\ny\n");
    state.toggle_focus();
    state.jump_to_next_hunk();
    assert_eq!(state.right.cursor().row, 1);
    assert_eq!(state.left.cursor().row, 0, "the unfocused pane's cursor shouldn't move");
}
