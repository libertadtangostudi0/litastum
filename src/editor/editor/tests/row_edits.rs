use super::*;
use crate::editor::editor::undo::Change;

fn editor_at(contents: &str, row: usize, col: usize) -> Editor {
    let (mut editor, _path) = open_test_editor(contents);
    editor.state.cursor = Index2::new(row, col);
    editor
}


/// Typing keeps one row per undo entry rather than a copy of the whole
/// buffer -- the copy cost ~170 ms per keystroke on 300k lines.
#[test]
fn typing_snapshots_only_the_row() {
    let mut editor = editor_at("aa\nbb\ncc\n", 1, 1);

    editor.input(key(KeyCode::Char('x')));

    assert!(matches!(editor.undo_stack.last().map(|snapshot| &snapshot.change), Some(Change::Row { row: 1, .. })));
    assert_eq!(editor.text(), "aa\nbxb\ncc\n");
}

#[test]
fn enter_and_a_row_joining_backspace_snapshot_the_whole_buffer() {
    let mut editor = editor_at("aa\nbb\n", 1, 0);

    editor.input(key(KeyCode::Backspace));
    assert!(matches!(editor.undo_stack.last().map(|snapshot| &snapshot.change), Some(Change::Buffer(_))), "joins rows 0 and 1");

    editor.input(key(KeyCode::Enter));
    assert!(matches!(editor.undo_stack.last().map(|snapshot| &snapshot.change), Some(Change::Buffer(_))));
}

/// Row entries and whole-buffer entries undo and redo in order.
#[test]
fn mixed_row_and_buffer_edits_undo_and_redo_in_order() {
    let mut editor = editor_at("ab\ncd\n", 0, 1);
    editor.input(key(KeyCode::Char('1')));
    editor.input(key(KeyCode::Enter));
    editor.input(key(KeyCode::Char('2')));
    editor.input(key(KeyCode::Delete));
    let edited = editor.text();
    assert_eq!(edited, "a1\n2\ncd\n");

    let mut undone = Vec::new();
    for _ in 0..4 {
        editor.input(ctrl_key('z'));
        undone.push(editor.text());
    }
    assert_eq!(undone, ["a1\n2b\ncd\n", "a1\nb\ncd\n", "a1b\ncd\n", "ab\ncd\n"]);

    for _ in 0..4 {
        editor.input(ctrl_key('y'));
    }
    assert_eq!(editor.text(), edited);
}

#[test]
fn typing_and_deleting_it_again_is_clean() {
    let mut editor = editor_at("abc\n", 0, 1);

    editor.input(key(KeyCode::Char('x')));
    assert!(editor.is_dirty());
    editor.input(key(KeyCode::Backspace));

    assert!(!editor.is_dirty(), "the buffer matches the file again");
}

#[test]
fn edits_on_two_rows_stay_dirty_until_both_are_undone() {
    let mut editor = editor_at("ab\ncd\n", 0, 0);
    editor.input(key(KeyCode::Char('x')));
    editor.state.cursor = Index2::new(1, 0);
    editor.input(key(KeyCode::Char('y')));

    editor.input(ctrl_key('z'));
    assert!(editor.is_dirty(), "row 0 still has the x");
    editor.input(ctrl_key('z'));
    assert!(!editor.is_dirty());
}

#[test]
fn a_row_edit_after_a_save_compares_with_what_was_saved() {
    let mut editor = editor_at("ab\n", 0, 0);
    editor.input(key(KeyCode::Char('x')));
    editor.save().unwrap();
    assert!(!editor.is_dirty());

    editor.input(key(KeyCode::Backspace));
    assert!(editor.is_dirty(), "the saved file has the x");
    editor.input(ctrl_key('z'));
    assert!(!editor.is_dirty());
}

#[test]
fn a_very_long_line_typed_into_turns_highlighting_off_and_back() {
    let long = "x".repeat(crate::editor::word_highlight::MAX_HIGHLIGHTED_LINE_LEN);
    let mut editor = editor_at(&format!("{long}\nshort\n"), 0, 0);
    assert!(!editor.has_long_line);

    editor.input(key(KeyCode::Char('y')));
    assert!(editor.has_long_line, "one character over the limit");
    editor.input(key(KeyCode::Backspace));
    assert!(!editor.has_long_line);
}

#[test]
fn a_one_line_paste_snapshots_only_the_row_and_undoes_in_one_step() {
    let mut editor = editor_at("ab\ncd\n", 1, 1);

    editor.paste_text("XYZ");

    assert!(matches!(editor.undo_stack.last().map(|snapshot| &snapshot.change), Some(Change::Row { row: 1, .. })));
    assert_eq!(editor.text(), "ab\ncXYZd\n");
    editor.input(ctrl_key('z'));
    assert_eq!(editor.text(), "ab\ncd\n");
    assert!(!editor.is_dirty());
}
