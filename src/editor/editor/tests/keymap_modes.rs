use super::*;


#[test]
fn open_starts_clean() {
    let (editor, _path) = open_test_editor("hello\n");
    assert!(!editor.is_dirty());
}


/// `Standard` starts in `Insert` -- typing immediately inserts, matching
/// every non-modal editor this app is modeled on.
#[test]
fn opening_in_standard_mode_starts_in_insert() {
    let (editor, _path) = open_test_editor("hello\n");
    assert_eq!(editor.state.mode, EditorMode::Insert);
    assert_eq!(editor.keymap_mode(), EditorKeymapMode::Standard);
}


/// `Vim` starts in `Normal`, matching real Vim's own convention and
/// `edtui`'s own `vim_mode()` binding table, which expects to begin
/// there (`starting_mode`'s own doc comment).
#[test]
fn opening_in_vim_mode_starts_in_normal() {
    let path = unique_scratch_dir("editor").join("file.txt");
    fs::write(&path, "hello\n").expect("write test fixture file");
    let editor = Editor::open(path, None, EditorKeymapMode::Vim).expect("open test fixture file");

    assert_eq!(editor.state.mode, EditorMode::Normal);
    assert_eq!(editor.keymap_mode(), EditorKeymapMode::Vim);
}


/// A real, behavioral difference between the two keymaps, not just a
/// data-field check: in `Vim`'s `Normal` mode, a plain letter is a
/// command, not literal text -- typing `x` there must *not* insert an
/// `x` into the buffer, unlike `Standard`'s own `Insert` mode where it
/// always does (`typing_marks_dirty` above).
#[test]
fn vim_normal_mode_does_not_insert_plain_characters() {
    let path = unique_scratch_dir("editor").join("file.txt");
    fs::write(&path, "hello\n").expect("write test fixture file");
    let mut editor = Editor::open(path, None, EditorKeymapMode::Vim).expect("open test fixture file");

    editor.input(key(KeyCode::Char('!')));

    assert!(!editor.is_dirty(), "a plain letter in Vim's Normal mode is a command, not inserted text");
}


/// No correction pass runs under `Vim`. Observable through the line wrap:
/// under `Standard`, `Left` at a line start wraps to the previous line;
/// under `Vim` a raw `Left` isn't bound at all, so nothing moves.
#[test]
fn line_boundary_wrapping_does_not_run_in_vim_mode() {
    let path = unique_scratch_dir("editor").join("file.txt");
    fs::write(&path, "first\nsecond").expect("write test fixture file");
    let mut editor = Editor::open(path, None, EditorKeymapMode::Vim).expect("open test fixture file");
    editor.state.cursor = Index2::new(1, 0); // start of the second line

    editor.input(key(KeyCode::Left));

    assert_eq!(editor.state.cursor, Index2::new(1, 0), "a raw Left arrow should have no effect in Vim mode, not wrap up a line");
}


/// Not a bug (asked with a screenshot): in Vim `Normal`, `Right`/`l` stops
/// on a line's last character (`max_col_normal`) and never wraps -- real
/// Vim behavior. "root = true" ends at column 10. History: docs/history/editor-keymap.md.
#[test]
fn vim_right_arrow_stops_at_the_last_character_of_a_line_not_a_bug() {
    let path = unique_scratch_dir("editor").join(".editorconfig");
    let content = "root = true\n\n[*]\ncharset = utf-8\nend_of_line = lf\n";
    fs::write(&path, content).expect("write test fixture file");
    let mut editor = Editor::open(path, None, EditorKeymapMode::Vim).expect("open test fixture file");

    for _ in 0..20 {
        editor.input(key(KeyCode::Right));
    }

    assert_eq!(
        editor.state.cursor,
        Index2::new(0, 10),
        "cursor should settle on column 10, the last real character ('e') of \"root = true\" (11 chars, indices 0..=10) -- \
         Normal mode's own max_col clamp, not a bug"
    );
}
