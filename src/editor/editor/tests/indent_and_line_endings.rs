//! Reported: after a save, VS Code showed the file's indentation broken.
//! A save must give back the file's own line breaks, and Tab must indent
//! the way the file does (`text_format.rs`, `indent.rs`).

use super::*;


fn saved(editor: &mut Editor, path: &PathBuf) -> String {
    editor.save().unwrap();
    fs::read_to_string(path).unwrap()
}


fn shift_tab() -> KeyEvent {
    KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)
}


#[test]
fn a_crlf_file_saved_untouched_is_byte_for_byte_the_same() {
    for text in ["a\r\n\tb\r\n    c\r\n", "a\r\nb", "\r\n\r\n", "\u{feff}x\r\n\ty\r\n"] {
        let (mut editor, path) = open_test_editor(text);
        assert_eq!(saved(&mut editor, &path), text);
    }
}


#[test]
fn an_lf_file_saved_untouched_is_byte_for_byte_the_same() {
    for text in ["a\n\tb\n    c\n", "a\nb", "\n", "", "one line"] {
        let (mut editor, path) = open_test_editor(text);
        assert_eq!(saved(&mut editor, &path), text);
    }
}


#[test]
fn a_crlf_file_keeps_crlf_after_edits() {
    let (mut editor, path) = open_test_editor("fn a() {\r\n    b();\r\n}\r\n");
    editor.input(key(KeyCode::Down));
    editor.input(key(KeyCode::End));
    editor.input(key(KeyCode::Enter));
    editor.input(key(KeyCode::Char('c')));
    editor.paste_text("\r\nd\r\ne");
    assert_eq!(saved(&mut editor, &path), "fn a() {\r\n    b();\r\n    c\r\nd\r\ne\r\n}\r\n", "Enter keeps the indent, a paste doesn't add one");
}


#[test]
fn an_lf_file_keeps_lf_after_a_crlf_paste() {
    let (mut editor, path) = open_test_editor("a\nb\n");
    editor.input(key(KeyCode::End));
    editor.paste_text("\r\nx\r\ny");
    assert_eq!(saved(&mut editor, &path), "a\nx\ny\nb\n");
}


#[test]
fn a_file_without_a_last_line_break_gets_none_added() {
    let (mut editor, path) = open_test_editor("a\r\nb");
    editor.input(key(KeyCode::Char('x')));
    assert_eq!(saved(&mut editor, &path), "xa\r\nb");
}


#[test]
fn save_as_keeps_the_line_break() {
    let (mut editor, path) = open_test_editor("a\r\nb\r\n");
    let other = path.with_file_name("other.txt");
    editor.save_as(other.clone()).unwrap();
    assert_eq!(fs::read_to_string(&other).unwrap(), "a\r\nb\r\n");
}


#[test]
fn tab_types_spaces_in_a_file_indented_with_spaces() {
    let (mut editor, path) = open_test_editor("fn a() {\n    b();\n}\n");
    editor.input(key(KeyCode::Down));
    editor.input(key(KeyCode::Down));
    editor.input(key(KeyCode::Tab));
    assert_eq!(saved(&mut editor, &path), "fn a() {\n    b();\n    }\n");
}


#[test]
fn tab_goes_to_the_next_stop_not_a_whole_level_further() {
    let (mut editor, path) = open_test_editor("a\n    b\n");
    editor.input(key(KeyCode::Char('x')));
    editor.input(key(KeyCode::Char('y')));
    editor.input(key(KeyCode::Tab));
    assert_eq!(editor.cursor(), Index2::new(0, 4));
    assert_eq!(saved(&mut editor, &path), "xy  a\n    b\n");
}


#[test]
fn tab_types_two_spaces_in_a_file_indented_by_two() {
    let (mut editor, path) = open_test_editor("a:\n  b:\n    c: 1\n");
    editor.input(key(KeyCode::Tab));
    assert_eq!(saved(&mut editor, &path), "  a:\n  b:\n    c: 1\n");
}


#[test]
fn tab_types_a_tab_in_a_file_indented_with_tabs() {
    let (mut editor, path) = open_test_editor("fn a() {\n\tb();\n}\n");
    editor.input(key(KeyCode::Down));
    editor.input(key(KeyCode::End));
    editor.input(key(KeyCode::Tab));
    assert_eq!(saved(&mut editor, &path), "fn a() {\n\tb();\t\n}\n");
}


#[test]
fn tab_in_an_empty_file_types_four_spaces() {
    let (mut editor, path) = open_test_editor("");
    editor.input(key(KeyCode::Tab));
    editor.input(key(KeyCode::Char('a')));
    assert_eq!(saved(&mut editor, &path), "    a");
}


#[test]
fn tab_undoes_in_one_step() {
    let (mut editor, path) = open_test_editor("a\n    b\n");
    editor.input(key(KeyCode::Tab));
    assert!(editor.is_dirty());
    editor.input(ctrl_key('z'));
    assert!(!editor.is_dirty(), "back as loaded");
    assert_eq!(editor.cursor(), Index2::new(0, 0));
    assert_eq!(saved(&mut editor, &path), "a\n    b\n");
}


#[test]
fn tab_over_a_selection_on_one_line_replaces_it() {
    let (mut editor, path) = open_test_editor("abc\n    d\n");
    editor.input(shift_key(KeyCode::Right));
    editor.input(shift_key(KeyCode::Right));
    editor.input(key(KeyCode::Tab));
    assert!(!editor.has_selection());
    assert_eq!(saved(&mut editor, &path), "    c\n    d\n");
}


#[test]
fn tab_over_several_lines_indents_each_and_keeps_the_selection() {
    let (mut editor, path) = open_test_editor("a\n\nb\nc\n    d\n");
    // From column 0, each `Shift+Down` takes one whole line.
    editor.input(shift_key(KeyCode::Down));
    editor.input(shift_key(KeyCode::Down));
    editor.input(shift_key(KeyCode::Down));
    editor.input(key(KeyCode::Tab));
    assert!(editor.has_selection(), "the lines stay selected for another Tab");
    editor.input(key(KeyCode::Tab));
    assert_eq!(saved(&mut editor, &path), "        a\n\n        b\nc\n    d\n", "an empty line isn't indented");
}


/// `Shift+Down` from a line's start selects it to its break: Tab indents
/// that line, as for several, rather than typing over it.
#[test]
fn tab_over_one_whole_line_indents_it() {
    let (mut editor, path) = open_test_editor("a\nb\n    c\n");
    editor.input(shift_key(KeyCode::Down));
    editor.input(key(KeyCode::Tab));
    assert!(editor.has_selection());
    assert_eq!(saved(&mut editor, &path), "    a\nb\n    c\n");
}


#[test]
fn shift_tab_over_one_whole_line_outdents_only_it() {
    let (mut editor, path) = open_test_editor("    a\n    b\n");
    editor.input(shift_key(KeyCode::Down));
    editor.input(shift_tab());
    assert_eq!(saved(&mut editor, &path), "a\n    b\n");
}


#[test]
fn indenting_several_lines_undoes_in_one_step() {
    let (mut editor, path) = open_test_editor("a\nb\n    c\n");
    editor.input(shift_key(KeyCode::Down));
    editor.input(shift_key(KeyCode::Down));
    editor.input(key(KeyCode::Tab));
    assert!(editor.is_dirty());
    editor.input(ctrl_key('z'));
    assert!(!editor.is_dirty());
    assert_eq!(saved(&mut editor, &path), "a\nb\n    c\n");
}


#[test]
fn shift_tab_takes_one_level_off_the_caret_line() {
    let (mut editor, path) = open_test_editor("a\n    b\n        c\n");
    editor.input(key(KeyCode::Down));
    editor.input(key(KeyCode::Down));
    editor.input(key(KeyCode::End));
    editor.input(shift_tab());
    assert_eq!(editor.cursor(), Index2::new(2, 5), "the caret moves with the text");
    assert_eq!(saved(&mut editor, &path), "a\n    b\n    c\n");
}


#[test]
fn shift_tab_as_tab_with_shift_does_the_same() {
    let (mut editor, path) = open_test_editor("    a\n");
    editor.input(shift_key(KeyCode::Tab));
    assert_eq!(saved(&mut editor, &path), "a\n");
}


#[test]
fn shift_tab_takes_a_tab_off_a_tab_indented_line() {
    let (mut editor, path) = open_test_editor("\t\ta\n\tb\n");
    editor.input(shift_tab());
    assert_eq!(saved(&mut editor, &path), "\ta\n\tb\n");
}


#[test]
fn shift_tab_on_an_unindented_line_changes_nothing() {
    let (mut editor, _path) = open_test_editor("a\n    b\n");
    editor.input(key(KeyCode::Right));
    editor.input(shift_tab());
    assert!(!editor.is_dirty());
    assert_eq!(editor.cursor(), Index2::new(0, 1));
}


#[test]
fn shift_tab_over_several_lines_outdents_each() {
    let (mut editor, path) = open_test_editor("    a\n        b\nc\n  d\n");
    editor.input(shift_key(KeyCode::Down));
    editor.input(shift_key(KeyCode::Down));
    editor.input(shift_key(KeyCode::Down));
    editor.input(shift_key(KeyCode::Down));
    editor.input(shift_tab());
    assert!(editor.has_selection());
    assert_eq!(saved(&mut editor, &path), "a\n    b\nc\nd\n");
}


#[test]
fn shift_tab_undoes_in_one_step() {
    let (mut editor, path) = open_test_editor("        a\n    b\n");
    editor.input(shift_tab());
    editor.input(shift_tab());
    editor.input(ctrl_key('z'));
    assert_eq!(saved(&mut editor, &path), "    a\n    b\n");
}


#[test]
fn indenting_a_crlf_file_keeps_crlf() {
    let (mut editor, path) = open_test_editor("a\r\n    b\r\nc\r\n");
    editor.input(shift_key(KeyCode::Down));
    editor.input(shift_key(KeyCode::Down));
    editor.input(shift_key(KeyCode::Down));
    editor.input(key(KeyCode::Tab));
    assert_eq!(saved(&mut editor, &path), "    a\r\n        b\r\n    c\r\n");
}


#[test]
fn shift_tab_is_the_editors_outdent_not_a_crash() {
    assert_eq!(crate::editor::editor_keymap::resolve(shift_tab()), crate::editor::editor_keymap::EditorCommand::Outdent);
}


#[test]
fn shift_tab_under_vim_is_ignored_not_a_crash() {
    let path = unique_scratch_dir("editor").join("file.txt");
    fs::write(&path, "    a\n").unwrap();
    let mut editor = Editor::open(path, None, EditorKeymapMode::Vim).unwrap();
    crate::editor::text_key(&mut editor, shift_tab());
    assert!(!editor.is_dirty());
}


/// The editor drawn, as text.
fn drawn(editor: &mut Editor) -> String {
    let theme = Theme::dark();
    crate::test_support::render_text(60, 6, |frame| {
        let view = editor.view(&theme, frame.area());
        frame.render_widget(view, frame.area());
    })
}


#[test]
fn the_bottom_border_shows_the_files_line_breaks() {
    let (mut editor, _path) = open_test_editor("a\r\nb\r\n");
    assert!(drawn(&mut editor).contains(" CRLF "));
    let (mut editor, _path) = open_test_editor("a\nb\n");
    assert!(drawn(&mut editor).contains(" LF "));
    let (mut editor, _path) = open_test_editor("a\r\nb\nc\n");
    assert!(drawn(&mut editor).contains(" Mixed "));
}


#[test]
fn picking_other_line_breaks_marks_the_file_modified_and_the_save_writes_them() {
    let (mut editor, path) = open_test_editor("a\r\n    b\r\n");
    editor.set_line_ending(LineEnding::Lf);
    assert!(editor.is_dirty());
    let screen = drawn(&mut editor);
    assert!(screen.contains("[modified]"), "{screen}");
    assert!(screen.contains(" LF "), "{screen}");

    assert_eq!(saved(&mut editor, &path), "a\n    b\n");
    assert!(!editor.is_dirty());
    assert_eq!(editor.line_ending_label(), "LF");
}


#[test]
fn lf_to_crlf_too() {
    let (mut editor, path) = open_test_editor("a\nb");
    editor.set_line_ending(LineEnding::CrLf);
    assert_eq!(saved(&mut editor, &path), "a\r\nb", "no line break added at the end");
}


#[test]
fn picking_the_files_own_line_breaks_again_is_no_change() {
    let (mut editor, _path) = open_test_editor("a\r\nb\r\n");
    editor.set_line_ending(LineEnding::Lf);
    editor.set_line_ending(LineEnding::CrLf);
    assert!(!editor.is_dirty());
}


#[test]
fn a_picked_line_break_survives_edits_and_undo() {
    let (mut editor, path) = open_test_editor("a\r\nb\r\n");
    editor.set_line_ending(LineEnding::Lf);
    editor.input(key(KeyCode::Char('x')));
    editor.input(ctrl_key('z'));
    assert!(editor.is_dirty(), "the text is back, the line breaks are still to change");
    assert_eq!(saved(&mut editor, &path), "a\nb\n");
}


#[test]
fn a_mixed_file_made_uniform_on_request() {
    let (mut editor, path) = open_test_editor("a\r\nb\nc\r\n");
    assert_eq!(editor.line_ending_label(), "Mixed");
    assert!(!editor.is_dirty(), "mixed isn't a change by itself");
    editor.set_line_ending(LineEnding::CrLf);
    assert!(editor.is_dirty(), "the dominant one, but the LF line changes");
    assert_eq!(saved(&mut editor, &path), "a\r\nb\r\nc\r\n");
    assert_eq!(editor.line_ending_label(), "CRLF");
}


/// Until line breaks are kept per line, saving an edited mixed file
/// writes the dominant one throughout -- the border said `Mixed` before.
#[test]
fn an_edited_mixed_file_is_saved_with_its_dominant_line_break() {
    let (mut editor, path) = open_test_editor("a\r\nb\nc\r\n");
    editor.input(key(KeyCode::Char('x')));
    assert_eq!(saved(&mut editor, &path), "xa\r\nb\r\nc\r\n");
    assert_eq!(editor.line_ending_label(), "CRLF");
}


#[test]
fn save_as_writes_the_picked_line_break() {
    let (mut editor, path) = open_test_editor("a\r\nb\r\n");
    editor.set_line_ending(LineEnding::Lf);
    let other = path.with_file_name("other.txt");
    editor.save_as(other.clone()).unwrap();
    assert_eq!(fs::read_to_string(&other).unwrap(), "a\nb\n");
}
