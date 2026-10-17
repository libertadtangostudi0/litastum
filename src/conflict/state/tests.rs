use std::path::Path;

use super::*;
use crate::conflict::detect;
use crate::test_support::unique_scratch_dir;

/// Writes the four conflict files into a fresh directory and returns
/// them in the order a panel lists them.
pub(crate) fn write_conflict_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let files = [
        ("a.txt", "one\n<<<<<<< .working\nmine\n=======\ntheirs\n>>>>>>> .merge-right.r2\n"),
        ("a.txt.merge-left.r1", "one\nbase\n"),
        ("a.txt.merge-right.r2", "one\ntheirs\n"),
        ("a.txt.working", "one\nmine\n"),
    ];
    files
        .iter()
        .map(|(name, content)| {
            std::fs::write(dir.join(name), content).unwrap();
            dir.join(name)
        })
        .collect()
}

pub(crate) fn open_conflict() -> ConflictState {
    let dir = unique_scratch_dir("conflict-state");
    let files = detect(&write_conflict_files(&dir)).unwrap();
    ConflictState::open(files, None, EditorKeymapMode::Standard).unwrap()
}

#[test]
fn opens_each_file_in_its_pane_focused_on_the_result() {
    let state = open_conflict();
    assert_eq!(state.focus, Pane::Result);
    assert_eq!(state.working.text(), "one\nmine\n");
    assert!(state.result.text().contains("<<<<<<< .working"));
    assert_eq!(state.theirs.text(), "one\ntheirs\n");
    assert_eq!(state.incoming.left.text(), "one\nbase\n");
    assert_eq!(state.incoming.right.text(), "one\ntheirs\n");
}

#[test]
fn tab_visits_every_pane_including_both_halves_of_the_bottom_compare() {
    let mut state = open_conflict();
    let mut visited = Vec::new();
    for _ in 0..FOCUS_STOPS {
        state.focus_next();
        visited.push((state.focus, state.incoming.focus));
    }
    assert_eq!(
        visited,
        [
            (Pane::Theirs, Side::Left),
            (Pane::Incoming, Side::Left),
            (Pane::Incoming, Side::Right),
            (Pane::Working, Side::Right),
            (Pane::Result, Side::Right),
        ]
    );
}

#[test]
fn shift_tab_goes_back_the_same_way() {
    let mut state = open_conflict();
    state.focus_previous();
    assert_eq!(state.focus, Pane::Working);
    state.focus_previous();
    assert_eq!((state.focus, state.incoming.focus), (Pane::Incoming, Side::Right));
    state.focus_previous();
    assert_eq!((state.focus, state.incoming.focus), (Pane::Incoming, Side::Left));
}

/// A conflict (rows 1-5), then "C" (row 7), which differs from both
/// sides' "c" outside any conflict.
fn open_with_a_conflict_and_a_change() -> ConflictState {
    let dir = unique_scratch_dir("conflict-jumps");
    let files = detect(&write_conflict_files(&dir)).unwrap();
    std::fs::write(&files.result, "a\n<<<<<<< .working\nm\n=======\nt\n>>>>>>> r\nb\nC\n").unwrap();
    std::fs::write(&files.working, "a\nm\nb\nc\n").unwrap();
    std::fs::write(&files.theirs, "a\nt\nb\nc\n").unwrap();
    ConflictState::open(files, None, EditorKeymapMode::Standard).unwrap()
}

/// Regression: F8 in the result used to stop only on each conflict's
/// `<<<<<<<`, skipping every other marker and every change outside
/// a conflict.
#[test]
fn f7_f8_in_the_result_stop_on_every_marker_and_every_change() {
    let mut state = open_with_a_conflict_and_a_change();

    let mut visited = Vec::new();
    for _ in 0..5 {
        state.jump_to_next();
        visited.push(state.result.cursor().row);
    }
    assert_eq!(visited, [1, 3, 5, 7, 7], "<<<<<<<, =======, >>>>>>>, \"C\", then no further stop");

    state.jump_to_previous();
    assert_eq!(state.result.cursor().row, 5);
    state.result.set_cursor(Index2::new(4, 0));
    state.jump_to_previous();
    assert_eq!(state.result.cursor().row, 3, "the ======= above \"t\"");
}

#[test]
fn f8_in_the_side_panes_steps_through_their_changes_against_the_result() {
    let mut state = open_with_a_conflict_and_a_change();

    state.focus = Pane::Working;
    state.jump_to_next();
    assert_eq!(state.working.cursor().row, 3, "\"c\" vs \"C\"; \"m\" is in the result too");

    state.focus = Pane::Theirs;
    state.jump_to_next();
    assert_eq!(state.theirs.cursor().row, 3);
}

#[test]
fn a_submitted_path_loads_into_the_focused_top_pane() {
    let mut state = open_conflict();
    let other = unique_scratch_dir("conflict-path-edit").join("other.txt");
    std::fs::write(&other, "x\n").unwrap();
    state.focus = Pane::Working;

    state.start_path_edit();
    state.path_edit.as_mut().unwrap().field.set_text(other.to_string_lossy().into_owned());
    state.path_edit_key(crate::test_support::key(crossterm::event::KeyCode::Enter)).unwrap();

    assert!(state.path_edit.is_none());
    assert_eq!(state.working.text(), "x\n");
    assert!(state.result.text().contains("<<<<<<<"), "the other panes stay");
}

#[test]
fn the_bottom_compare_edits_its_own_path() {
    let mut state = open_conflict();
    state.focus_next();
    state.focus_next();
    assert_eq!(state.focus, Pane::Incoming);

    state.start_path_edit();

    assert!(state.is_editing_path());
    assert!(state.incoming.path_edit.is_some());
    assert!(state.path_edit.is_none());
}

/// The unfocused panes are scrolled into line every frame, which moves
/// their cursors; focusing one again brings its own cursor back.
#[test]
fn each_top_pane_gets_its_cursor_back_when_focused_again() {
    let mut state = open_conflict();
    state.focus_previous();
    assert_eq!(state.focus, Pane::Working);
    state.working.set_cursor(Index2::new(1, 2));

    state.focus_next();
    state.working.set_viewport_top_row(0);
    state.focus_previous();

    assert_eq!(state.working.cursor(), Index2::new(1, 2));
}

#[test]
fn is_dirty_reflects_any_pane() {
    let mut state = open_conflict();
    assert!(!state.is_dirty());
    state.incoming.right.input(crate::test_support::key(crossterm::event::KeyCode::Char('!')));
    assert!(state.is_dirty());
}
