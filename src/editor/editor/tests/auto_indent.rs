//! Enter keeps the line's indentation; Backspace in spaces takes a level
//! (`auto_indent.rs`). Requested: Enter started every line at column 0.

use super::*;


fn saved(editor: &mut Editor, path: &PathBuf) -> String {
    editor.save().unwrap();
    fs::read_to_string(path).unwrap()
}


/// Opens `contents` under `name` (the extension picks the `:` rule).
fn open_named(name: &str, contents: &str) -> (Editor, PathBuf) {
    let path = unique_scratch_dir("editor").join(name);
    fs::write(&path, contents).unwrap();
    (Editor::open(path.clone(), None, EditorKeymapMode::Standard).unwrap(), path)
}


fn press(editor: &mut Editor, codes: &[KeyCode]) {
    for &code in codes {
        editor.input(key(code));
    }
}


fn type_text(editor: &mut Editor, text: &str) {
    for c in text.chars() {
        editor.input(key(KeyCode::Char(c)));
    }
}


#[test]
fn enter_keeps_the_lines_spaces() {
    let (mut editor, path) = open_test_editor("    a\n");
    press(&mut editor, &[KeyCode::End, KeyCode::Enter]);
    assert_eq!(editor.cursor(), Index2::new(1, 4));
    type_text(&mut editor, "b");
    assert_eq!(saved(&mut editor, &path), "    a\n    b\n");
}


#[test]
fn enter_keeps_the_lines_tabs() {
    let (mut editor, path) = open_test_editor("\t\ta\n");
    press(&mut editor, &[KeyCode::End, KeyCode::Enter]);
    type_text(&mut editor, "b");
    assert_eq!(saved(&mut editor, &path), "\t\ta\n\t\tb\n");
}


/// In a mixed file the new line copies the line's own characters, not
/// the file's style.
#[test]
fn enter_copies_a_mixed_indent_as_it_is() {
    let (mut editor, path) = open_test_editor("    x\n    y\n\t  a\n");
    press(&mut editor, &[KeyCode::Down, KeyCode::Down, KeyCode::End, KeyCode::Enter]);
    type_text(&mut editor, "b");
    assert_eq!(saved(&mut editor, &path), "    x\n    y\n\t  a\n\t  b\n");
}


#[test]
fn enter_on_an_unindented_line_starts_at_column_zero() {
    let (mut editor, path) = open_test_editor("a\n");
    press(&mut editor, &[KeyCode::End, KeyCode::Enter]);
    assert_eq!(editor.cursor(), Index2::new(1, 0));
    assert_eq!(saved(&mut editor, &path), "a\n\n");
}


#[test]
fn enter_after_a_brace_adds_a_level() {
    let (mut editor, path) = open_test_editor("fn a() {\n    b();\n}\n");
    press(&mut editor, &[KeyCode::Down, KeyCode::End, KeyCode::Enter]);
    type_text(&mut editor, "if c {");
    press(&mut editor, &[KeyCode::Enter]);
    assert_eq!(editor.cursor(), Index2::new(3, 8));
    type_text(&mut editor, "d();");
    assert_eq!(saved(&mut editor, &path), "fn a() {\n    b();\n    if c {\n        d();\n}\n");
}


#[test]
fn enter_after_an_opening_bracket_or_parenthesis_adds_a_level() {
    for opener in ['(', '['] {
        let (mut editor, path) = open_test_editor("    x\n");
        press(&mut editor, &[KeyCode::End]);
        type_text(&mut editor, &opener.to_string());
        press(&mut editor, &[KeyCode::Enter]);
        assert_eq!(saved(&mut editor, &path), format!("    x{opener}\n        \n"), "{opener}");
    }
}


#[test]
fn trailing_spaces_after_the_brace_dont_hide_it() {
    let (mut editor, path) = open_test_editor("a {  \n    b\n");
    press(&mut editor, &[KeyCode::End, KeyCode::Enter]);
    assert_eq!(editor.cursor(), Index2::new(1, 4));
    type_text(&mut editor, "x");
    assert_eq!(saved(&mut editor, &path), "a {  \n    x\n    b\n");
}


#[test]
fn enter_between_braces_puts_the_closer_a_line_further_down() {
    let (mut editor, path) = open_test_editor("    a\n");
    press(&mut editor, &[KeyCode::End]);
    type_text(&mut editor, " {}");
    press(&mut editor, &[KeyCode::Left, KeyCode::Enter]);
    assert_eq!(editor.cursor(), Index2::new(1, 8));
    type_text(&mut editor, "b");
    assert_eq!(saved(&mut editor, &path), "    a {\n        b\n    }\n");
}


#[test]
fn enter_between_mismatched_brackets_doesnt_split_off_the_closer() {
    let (mut editor, path) = open_test_editor("    a\n");
    press(&mut editor, &[KeyCode::End]);
    type_text(&mut editor, "(]");
    press(&mut editor, &[KeyCode::Left, KeyCode::Enter]);
    assert_eq!(saved(&mut editor, &path), "    a(\n        ]\n");
}


#[test]
fn enter_with_tabs_adds_a_tab_after_a_brace() {
    let (mut editor, path) = open_test_editor("\tif a {\n\t\tb\n\t}\n");
    press(&mut editor, &[KeyCode::End, KeyCode::Enter]);
    type_text(&mut editor, "c");
    assert_eq!(saved(&mut editor, &path), "\tif a {\n\t\tc\n\t\tb\n\t}\n");
}


#[test]
fn a_colon_opens_a_block_in_python_and_yaml_only() {
    let (mut editor, path) = open_named("a.py", "def a():\n    pass\n");
    press(&mut editor, &[KeyCode::End, KeyCode::Enter]);
    type_text(&mut editor, "x");
    assert_eq!(saved(&mut editor, &path), "def a():\n    x\n    pass\n");

    let (mut editor, path) = open_named("a.yaml", "a:\n  b: 1\n");
    press(&mut editor, &[KeyCode::End, KeyCode::Enter]);
    type_text(&mut editor, "c: 2");
    assert_eq!(saved(&mut editor, &path), "a:\n  c: 2\n  b: 1\n");

    let (mut editor, path) = open_named("a.rs", "    x:\n");
    press(&mut editor, &[KeyCode::End, KeyCode::Enter]);
    type_text(&mut editor, "y");
    assert_eq!(saved(&mut editor, &path), "    x:\n    y\n", "a label, not a block");
}


#[test]
fn enter_in_the_middle_of_a_line_drops_the_spaces_at_the_split() {
    let (mut editor, path) = open_test_editor("    a   b\n");
    press(&mut editor, &[KeyCode::Home]);
    for _ in 0..5 {
        editor.input(key(KeyCode::Right));
    }
    press(&mut editor, &[KeyCode::Enter]);
    assert_eq!(editor.cursor(), Index2::new(1, 4));
    assert_eq!(saved(&mut editor, &path), "    a\n    b\n");
}


#[test]
fn enter_at_the_indentation_moves_the_line_down_whole() {
    let (mut editor, path) = open_test_editor("    a\n");
    for _ in 0..4 {
        editor.input(key(KeyCode::Right));
    }
    press(&mut editor, &[KeyCode::Enter]);
    assert_eq!(editor.cursor(), Index2::new(1, 4));
    assert_eq!(saved(&mut editor, &path), "\n    a\n", "no spaces left behind");
}


#[test]
fn enter_at_a_lines_start_moves_it_down_whole() {
    let (mut editor, path) = open_test_editor("    a\n");
    press(&mut editor, &[KeyCode::Enter]);
    assert_eq!(editor.cursor(), Index2::new(1, 0));
    assert_eq!(saved(&mut editor, &path), "\n    a\n");
}


#[test]
fn enter_twice_leaves_no_trailing_whitespace() {
    let (mut editor, path) = open_test_editor("    a\n");
    press(&mut editor, &[KeyCode::End, KeyCode::Enter, KeyCode::Enter]);
    type_text(&mut editor, "b");
    assert_eq!(saved(&mut editor, &path), "    a\n\n    b\n");
}


#[test]
fn enter_over_a_selection_replaces_it() {
    let (mut editor, path) = open_test_editor("    abc\n");
    press(&mut editor, &[KeyCode::End]);
    editor.input(shift_key(KeyCode::Left));
    editor.input(shift_key(KeyCode::Left));
    press(&mut editor, &[KeyCode::Enter]);
    assert!(!editor.has_selection());
    assert_eq!(saved(&mut editor, &path), "    a\n    \n");
}


#[test]
fn enter_in_an_empty_file() {
    let (mut editor, path) = open_test_editor("");
    press(&mut editor, &[KeyCode::Enter]);
    type_text(&mut editor, "a");
    let line_break = if cfg!(windows) { "\r\n" } else { "\n" };
    assert_eq!(saved(&mut editor, &path), format!("{line_break}a"));
}


#[test]
fn enter_undoes_in_one_step() {
    let (mut editor, path) = open_test_editor("    a {}\n");
    press(&mut editor, &[KeyCode::End, KeyCode::Left, KeyCode::Enter]);
    editor.input(ctrl_key('z'));
    assert!(!editor.is_dirty());
    assert_eq!(editor.cursor(), Index2::new(0, 7));
    assert_eq!(saved(&mut editor, &path), "    a {}\n");
}


#[test]
fn enter_with_shift_does_the_same() {
    let (mut editor, path) = open_test_editor("    a\n");
    press(&mut editor, &[KeyCode::End]);
    editor.input(shift_key(KeyCode::Enter));
    type_text(&mut editor, "b");
    assert_eq!(saved(&mut editor, &path), "    a\n    b\n");
}


#[test]
fn backspace_in_spaces_takes_a_level() {
    let (mut editor, path) = open_test_editor("        a\n    b\n");
    for _ in 0..8 {
        editor.input(key(KeyCode::Right));
    }
    press(&mut editor, &[KeyCode::Backspace]);
    assert_eq!(editor.cursor(), Index2::new(0, 4));
    press(&mut editor, &[KeyCode::Backspace]);
    assert_eq!(saved(&mut editor, &path), "a\n    b\n");
}


#[test]
fn backspace_goes_back_to_the_previous_stop() {
    let (mut editor, path) = open_test_editor("      a\n    b\n");
    for _ in 0..6 {
        editor.input(key(KeyCode::Right));
    }
    press(&mut editor, &[KeyCode::Backspace]);
    assert_eq!(saved(&mut editor, &path), "    a\n    b\n");
}


#[test]
fn backspace_after_tab_undoes_its_spaces_in_one_press() {
    let (mut editor, path) = open_test_editor("a\n    b\n");
    press(&mut editor, &[KeyCode::Tab, KeyCode::Backspace]);
    assert!(!editor.is_dirty());
    assert_eq!(saved(&mut editor, &path), "a\n    b\n");
}


#[test]
fn backspace_in_the_middle_of_spaces_takes_them_to_the_stop() {
    let (mut editor, path) = open_test_editor("        a\n    b\n");
    for _ in 0..6 {
        editor.input(key(KeyCode::Right));
    }
    press(&mut editor, &[KeyCode::Backspace]);
    assert_eq!(editor.cursor(), Index2::new(0, 4));
    assert_eq!(saved(&mut editor, &path), "      a\n    b\n");
}


#[test]
fn backspace_after_text_deletes_one_character() {
    let (mut editor, path) = open_test_editor("    a    \n    b\n");
    press(&mut editor, &[KeyCode::End, KeyCode::Backspace]);
    assert_eq!(saved(&mut editor, &path), "    a   \n    b\n");
}


#[test]
fn backspace_after_a_tab_deletes_the_tab() {
    let (mut editor, path) = open_test_editor("\t\ta\n");
    press(&mut editor, &[KeyCode::Right, KeyCode::Right, KeyCode::Backspace]);
    assert_eq!(saved(&mut editor, &path), "\ta\n");
}


#[test]
fn backspace_after_one_space_deletes_it() {
    let (mut editor, path) = open_test_editor(" a\n    b\n");
    press(&mut editor, &[KeyCode::Right, KeyCode::Backspace]);
    assert_eq!(saved(&mut editor, &path), "a\n    b\n");
}


#[test]
fn backspace_at_a_lines_start_still_joins_it_to_the_one_above() {
    let (mut editor, path) = open_test_editor("a\n    b\n");
    press(&mut editor, &[KeyCode::Down, KeyCode::Home, KeyCode::Backspace]);
    assert_eq!(saved(&mut editor, &path), "a    b\n");
}


#[test]
fn backspace_over_a_selection_deletes_the_selection() {
    let (mut editor, path) = open_test_editor("        a\n");
    for _ in 0..8 {
        editor.input(key(KeyCode::Right));
    }
    editor.input(shift_key(KeyCode::Left));
    press(&mut editor, &[KeyCode::Backspace]);
    assert_eq!(saved(&mut editor, &path), "       a\n");
}


#[test]
fn backspace_in_a_two_space_file_takes_two() {
    let (mut editor, path) = open_named("a.yaml", "a:\n  b:\n    c: 1\n");
    press(&mut editor, &[KeyCode::Down, KeyCode::Down]);
    for _ in 0..4 {
        editor.input(key(KeyCode::Right));
    }
    press(&mut editor, &[KeyCode::Backspace]);
    assert_eq!(saved(&mut editor, &path), "a:\n  b:\n  c: 1\n");
}


#[test]
fn backspace_of_a_level_undoes_in_one_step() {
    let (mut editor, path) = open_test_editor("        a\n");
    for _ in 0..8 {
        editor.input(key(KeyCode::Right));
    }
    press(&mut editor, &[KeyCode::Backspace]);
    editor.input(ctrl_key('z'));
    assert!(!editor.is_dirty());
    assert_eq!(saved(&mut editor, &path), "        a\n");
}


#[test]
fn vim_keeps_its_own_enter_and_backspace() {
    let path = unique_scratch_dir("editor").join("file.txt");
    fs::write(&path, "    a\n").unwrap();
    let mut editor = Editor::open(path.clone(), None, EditorKeymapMode::Vim).unwrap();
    editor.input(shift_key(KeyCode::Char('A')));
    press(&mut editor, &[KeyCode::Enter, KeyCode::Char('b'), KeyCode::Esc]);
    editor.save().unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "    a\nb\n", "edtui's Enter, unindented");
}
