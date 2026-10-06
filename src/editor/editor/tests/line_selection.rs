use super::*;

/// What a `Backspace` leaves -- the selection's exact extent, the way a
/// cut or a paste over it would remove it.
fn text_after_deleting_the_selection(editor: &mut Editor) -> String {
    editor.input(key(KeyCode::Backspace));
    editor.text()
}

fn editor_at(contents: &str, row: usize, col: usize) -> Editor {
    let (mut editor, _path) = open_test_editor(contents);
    editor.state.cursor = Index2::new(row, col);
    editor
}


/// Reported: Shift+Down from a line's start also took the next line's
/// first character ("c" of "cc"), so a cut or paste ate it.
#[test]
fn shift_down_from_a_line_start_selects_the_whole_line_only() {
    let mut editor = editor_at("a\nbb\ncc\nd\n", 1, 0);
    editor.input(shift_key(KeyCode::Down));

    assert_eq!(text_after_deleting_the_selection(&mut editor), "a\ncc\nd\n");
}


#[test]
fn each_further_shift_down_adds_one_whole_line() {
    let mut editor = editor_at("a\nbb\ncc\nd\n", 1, 0);
    editor.input(shift_key(KeyCode::Down));
    editor.input(shift_key(KeyCode::Down));

    assert_eq!(text_after_deleting_the_selection(&mut editor), "a\nd\n");
}


#[test]
fn shift_up_takes_a_whole_line_back_off() {
    let mut editor = editor_at("a\nbb\ncc\nd\n", 1, 0);
    editor.input(shift_key(KeyCode::Down));
    editor.input(shift_key(KeyCode::Down));
    editor.input(shift_key(KeyCode::Up));

    assert_eq!(text_after_deleting_the_selection(&mut editor), "a\ncc\nd\n");
}


#[test]
fn shift_up_back_to_the_start_ends_the_selection_on_the_line_start() {
    let mut editor = editor_at("a\nbb\ncc\nd\n", 1, 0);
    editor.input(shift_key(KeyCode::Down));
    editor.input(shift_key(KeyCode::Up));

    assert!(!editor.has_selection());
    assert_eq!(editor.cursor(), Index2::new(1, 0));
}


/// The mirror case: Shift+Up from a line's start used to take that
/// line's first character too.
#[test]
fn shift_up_from_a_line_start_selects_the_whole_line_above_only() {
    let mut editor = editor_at("a\nbb\ncc\nd\n", 2, 0);
    editor.input(shift_key(KeyCode::Up));

    assert_eq!(text_after_deleting_the_selection(&mut editor), "a\ncc\nd\n");
}


#[test]
fn shift_down_back_to_the_start_ends_an_upward_line_selection() {
    let mut editor = editor_at("a\nbb\ncc\nd\n", 2, 0);
    editor.input(shift_key(KeyCode::Up));
    editor.input(shift_key(KeyCode::Up));
    editor.input(shift_key(KeyCode::Down));
    assert_eq!(editor.text(), "a\nbb\ncc\nd\n");
    assert!(editor.has_selection(), "still one line selected");

    editor.input(shift_key(KeyCode::Down));

    assert!(!editor.has_selection());
    assert_eq!(editor.cursor(), Index2::new(2, 0));
}


#[test]
fn a_paste_over_selected_lines_replaces_exactly_them() {
    let mut editor = editor_at("a\nbb\ncc\nd\n", 1, 0);
    editor.input(shift_key(KeyCode::Down));

    editor.paste_text("X\n");

    assert_eq!(editor.text(), "a\nX\ncc\nd\n");
}


/// Reported: Shift+End and Shift+Home dropped the selection and only
/// moved the cursor.
#[test]
fn shift_end_selects_to_the_end_of_the_line() {
    let mut editor = editor_at("hello world\n", 0, 6);
    editor.input(shift_key(KeyCode::End));

    assert_eq!(text_after_deleting_the_selection(&mut editor), "hello \n");
}


#[test]
fn shift_home_selects_to_the_start_without_the_character_under_the_caret() {
    let mut editor = editor_at("hello world\n", 0, 6);
    editor.input(shift_key(KeyCode::Home));

    assert_eq!(text_after_deleting_the_selection(&mut editor), "world\n");
}


#[test]
fn shift_end_extends_an_existing_selection() {
    let mut editor = editor_at("hello world\n", 0, 0);
    editor.input(shift_key(KeyCode::Right));
    editor.input(shift_key(KeyCode::End));

    assert_eq!(text_after_deleting_the_selection(&mut editor), "\n");
}


#[test]
fn shift_home_at_the_start_and_shift_end_at_the_end_select_nothing() {
    let mut editor = editor_at("hello\n", 0, 0);
    editor.input(shift_key(KeyCode::Home));
    assert!(!editor.has_selection());
    assert_eq!(editor.cursor(), Index2::new(0, 0));

    let mut editor = editor_at("hello\n", 0, 5);
    editor.input(shift_key(KeyCode::End));
    assert!(!editor.has_selection());
    assert_eq!(editor.cursor(), Index2::new(0, 5));
}



/// `edtui`'s own delete removed a fully selected line together with its
/// line break, so a paste over it ran into the next line ("Xnext").
#[test]
fn a_paste_over_a_fully_selected_line_keeps_the_next_line_apart() {
    let mut editor = editor_at("hello world
next
", 0, 0);
    editor.input(shift_key(KeyCode::End));

    editor.paste_text("X");

    assert_eq!(editor.text(), "X
next
");
}


#[test]
fn backspace_on_a_fully_selected_line_leaves_it_empty() {
    let mut editor = editor_at("ab
hello world
next
", 1, 0);
    editor.input(shift_key(KeyCode::End));

    assert_eq!(text_after_deleting_the_selection(&mut editor), "ab

next
");
    assert_eq!(editor.cursor(), Index2::new(1, 0));
}


#[test]
fn typing_over_a_fully_selected_line_replaces_just_its_text() {
    let mut editor = editor_at("hello
next
", 0, 0);
    editor.input(shift_key(KeyCode::End));

    editor.input(key(KeyCode::Char('x')));

    assert_eq!(editor.text(), "x
next
");
}
