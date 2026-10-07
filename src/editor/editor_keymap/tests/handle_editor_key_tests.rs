use super::*;

#[test]
fn handle_editor_key_ctrl_s_saves_and_clears_dirty() {
    let (mut app, path) = open_editor_app("hi\n");
    handle_editor_key(&mut app, ctrl_key('c')).ok(); // no-op sanity: forwarded, doesn't save
    handle_editor_key(&mut app, key(KeyCode::Char('!'))).unwrap();

    handle_editor_key(&mut app, ctrl_key('s')).unwrap();

    let Mode::Editing(editor) = &app.mode else { panic!("expected Mode::Editing") };
    assert!(!editor.is_dirty());
    assert_eq!(fs::read_to_string(&path).unwrap(), "!hi\n");
}

/// Requested: Ctrl+Down/Ctrl+Up move the caret between blocks of code.
#[test]
fn ctrl_down_and_up_move_between_blocks() {
    let (mut app, _path) = open_editor_app("a
b

c
");

    handle_editor_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL)).unwrap();
    let Mode::Editing(editor) = &app.mode else { panic!("expected Mode::Editing") };
    assert_eq!(editor.cursor_row(), 3);

    handle_editor_key(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL)).unwrap();
    let Mode::Editing(editor) = &app.mode else { panic!("expected Mode::Editing") };
    assert_eq!(editor.cursor_row(), 0);
    assert!(!editor.is_dirty());
}

/// F2 saves, as in Far's editor.
#[test]
fn f2_saves_like_ctrl_s() {
    let (mut app, path) = open_editor_app("hi
");
    handle_editor_key(&mut app, key(KeyCode::Char('!'))).unwrap();

    handle_editor_key(&mut app, key(KeyCode::F(2))).unwrap();

    let Mode::Editing(editor) = &app.mode else { panic!("expected Mode::Editing") };
    assert!(!editor.is_dirty());
    assert_eq!(fs::read_to_string(&path).unwrap(), "!hi
");
}

/// Shift+F2 turns the title into a path field; Enter saves there and the
/// editor carries on with the new file. Esc first leaves the field only.
#[test]
fn shift_f2_saves_as_a_typed_path() {
    let (mut app, path) = open_editor_app("hi
");
    handle_editor_key(&mut app, key(KeyCode::Char('!'))).unwrap();

    handle_editor_key(&mut app, KeyEvent::new(KeyCode::F(2), KeyModifiers::SHIFT)).unwrap();
    let target = path.with_file_name("copy.txt");
    app.editor_save_as.as_mut().expect("the field is open").field.set_text(target.to_string_lossy().into_owned());
    handle_editor_key(&mut app, key(KeyCode::Enter)).unwrap();

    assert!(app.editor_save_as.is_none());
    assert_eq!(fs::read_to_string(&target).unwrap(), "!hi
");
    assert_eq!(fs::read_to_string(&path).unwrap(), "hi
", "the original is untouched");
    let Mode::Editing(editor) = &app.mode else { panic!("expected Mode::Editing") };
    assert_eq!(editor.path(), target);
    assert!(!editor.is_dirty());
}

#[test]
fn esc_in_the_save_as_field_keeps_the_editor_open() {
    let (mut app, _path) = open_editor_app("hi
");
    handle_editor_key(&mut app, KeyEvent::new(KeyCode::F(2), KeyModifiers::SHIFT)).unwrap();
    handle_editor_key(&mut app, key(KeyCode::Char('x'))).unwrap();

    handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();

    assert!(app.editor_save_as.is_none());
    let Mode::Editing(editor) = &app.mode else { panic!("Esc closed the editor") };
    assert!(!editor.is_dirty(), "the x went into the field");
}

/// A save that fails (here: a read-only file) shows an error toast and
/// keeps the editor open; it used to return `Err` and end the app.
#[test]
fn a_failed_save_shows_a_notice_instead_of_ending_the_app() {
    let (mut app, path) = open_editor_app("hi\n");
    handle_editor_key(&mut app, key(KeyCode::Char('!'))).unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions.clone()).unwrap();

    let result = handle_editor_key(&mut app, ctrl_key('s'));

    permissions.set_readonly(false);
    fs::set_permissions(&path, permissions).unwrap();
    assert!(result.is_ok(), "a failed save must not end the app");
    let Some(notice) = &app.notice else { panic!("expected an error notice") };
    assert_eq!(notice.kind, crate::notice::NoticeKind::Error);
    assert!(notice.text.starts_with("Save failed"), "{notice:?}");
    let Mode::Editing(editor) = &app.mode else { panic!("the editor stays open") };
    assert!(editor.is_dirty(), "nothing was saved");
}

#[test]
fn handle_editor_key_plain_char_is_forwarded_and_marks_dirty() {
    let (mut app, _path) = open_editor_app("hi\n");

    handle_editor_key(&mut app, key(KeyCode::Char('x'))).unwrap();

    let Mode::Editing(editor) = &app.mode else { panic!("expected Mode::Editing") };
    assert!(editor.is_dirty());
}

#[test]
fn handle_editor_key_esc_with_no_changes_closes_straight_to_browsing() {
    let (mut app, _path) = open_editor_app("hi\n");

    handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();

    assert!(matches!(app.mode, Mode::Browsing));
}

#[test]
fn handle_editor_key_esc_with_unsaved_changes_asks_to_confirm_discard() {
    let (mut app, _path) = open_editor_app("hi\n");
    handle_editor_key(&mut app, key(KeyCode::Char('!'))).unwrap();

    handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();

    assert!(matches!(app.overlay, Some(Overlay::ConfirmDiscard)));
    assert!(matches!(app.mode, Mode::Editing(_)), "the editor stays open under the prompt");
}

/// Regression test for the real, reported crash: `cargo run` ->
/// open a file -> `F4` -> `F10` panicked the whole process
/// (`edtui`'s own `KeyCode::from` conversion has no arm for `F10`
/// at all). Must stay open, in `Mode::Editing`, completely
/// unaffected -- the editor's key handling is isolated from
/// whatever `F10` means on the browsing screen (global quit),
/// exactly as requested.
#[test]
fn handle_editor_key_f10_does_not_crash_or_close_the_editor() {
    let (mut app, _path) = open_editor_app("hi\n");

    handle_editor_key(&mut app, KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE)).unwrap();

    assert!(matches!(app.mode, Mode::Editing(_)), "F10 must not close the editor or crash while editing");
}

/// Regression coverage for the real request: `Ctrl+A` should select
/// the whole buffer -- verified functionally (deleting the
/// selection clears everything) rather than asserting on exact
/// cursor coordinates, which would be tied to `edtui`'s own
/// row/column indexing details.
#[test]
fn handle_editor_key_ctrl_a_selects_the_entire_buffer() {
    let (mut app, path) = open_editor_app("hello\nworld\n");

    handle_editor_key(&mut app, ctrl_key('a')).unwrap();
    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert!(editor.has_selection(), "Ctrl+A should open a selection");

    handle_editor_key(&mut app, key(KeyCode::Backspace)).unwrap();
    let Mode::Editing(active_editor) = &mut app.mode else { unreachable!() };
    active_editor.save().unwrap();

    assert_eq!(fs::read_to_string(&path).unwrap(), "", "deleting a Ctrl+A selection should clear the whole buffer");
}

/// `Ctrl+A`, `Backspace`, one `Ctrl+Z` restores everything -- a second
/// undo checkpoint used to need a second `Ctrl+Z`. History: docs/history/editor-keymap.md.
#[test]
fn handle_editor_key_ctrl_z_undoes_a_select_all_delete_in_one_press() {
    let (mut app, path) = open_editor_app("hello\nworld\n");
    handle_editor_key(&mut app, ctrl_key('a')).unwrap();
    handle_editor_key(&mut app, key(KeyCode::Backspace)).unwrap();

    handle_editor_key(&mut app, ctrl_key('z')).unwrap();

    let Mode::Editing(active_editor) = &mut app.mode else { unreachable!() };
    active_editor.save().unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "hello\nworld\n", "a single Ctrl+Z should restore everything the Ctrl+A/Backspace deleted");
}

#[test]
fn handle_editor_key_esc_with_an_active_selection_cancels_the_selection_instead_of_closing() {
    let (mut app, _path) = open_editor_app("hello\n");
    handle_editor_key(&mut app, shift_key(KeyCode::Right)).unwrap();
    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert!(editor.has_selection(), "precondition: a selection should be active");

    handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();

    let Mode::Editing(editor) = &app.mode else {
        panic!("Esc should cancel the selection, not close the editor");
    };
    assert!(!editor.has_selection());
}

#[test]
fn handle_editor_key_f9_opens_the_editor_menu_over_the_editor() {
    let (mut app, _path) = open_editor_app("hi\n");

    handle_editor_key(&mut app, key(KeyCode::F(9))).unwrap();

    let Mode::Editing(editor) = &app.mode else { panic!("expected Mode::Editing") };
    let Some(Overlay::EditorMenu(menu)) = &app.overlay else { panic!("expected the editor menu overlay") };
    assert_eq!(editor.keymap_mode(), EditorKeymapMode::Standard, "should carry the editor's own current mode over unchanged");
    assert_eq!(menu.selected_index(), 0, "should open at the first item (Keybindings)");
}

/// `F9` works in the editor+preview split view too -- it used to be
/// swallowed there, since the split view couldn't draw the menu
/// until overlays were drawn over any screen.
#[test]
fn handle_editor_key_f9_opens_the_menu_over_a_linked_markdown_preview_too() {
    let (mut app, dir_path) = open_editor_app("hi\n");
    let md_path = dir_path.with_file_name("preview.md");
    fs::write(&md_path, "# heading\n").expect("write markdown fixture");
    app.markdown_edit_preview = crate::explorer::MarkdownPreviewState::open(&md_path);
    assert!(app.markdown_edit_preview.is_some(), "precondition: the linked preview should have opened");

    handle_editor_key(&mut app, key(KeyCode::F(9))).unwrap();

    assert!(matches!(app.overlay, Some(Overlay::EditorMenu(_))));
}

/// `Ctrl+S` must still save and clear `is_dirty` correctly when the
/// edit was made through Vim's own commands (`x`, delete-under-cursor)
/// rather than typed characters -- and Vim's own `u` (Undo),
/// deliberately *not* in `can_mutate_buffer`'s small `hjkl` exemption
/// list, must correctly re-detect a return to the exact saved state
/// as no-longer-dirty.
#[test]
fn handle_editor_key_vim_delete_undo_and_save_all_keep_is_dirty_correct() {
    let dir = unique_scratch_dir("editor-keymap");
    let file_path = dir.join("file.txt");
    fs::write(&file_path, "hix\n").expect("write test fixture file");
    let editor = Editor::open(file_path.clone(), None, EditorKeymapMode::Vim).expect("open test fixture file");
    let mut app = test_app(dir);
    app.mode = Mode::Editing(editor);

    handle_editor_key(&mut app, key(KeyCode::Char('x'))).unwrap(); // deletes 'h' under the cursor
    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert!(editor.is_dirty(), "Vim's own 'x' should have marked the file dirty");

    handle_editor_key(&mut app, key(KeyCode::Char('u'))).unwrap(); // undo
    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert!(!editor.is_dirty(), "undoing back to the exact saved content should clear dirty");

    handle_editor_key(&mut app, key(KeyCode::Char('x'))).unwrap(); // redo the delete for real this time
    handle_editor_key(&mut app, ctrl_key('s')).unwrap();
    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert!(!editor.is_dirty(), "saving should clear dirty");
    assert_eq!(fs::read_to_string(&file_path).unwrap(), "ix\n");
}

/// `Esc` with a Vim `Visual` selection (from `v`) clears it rather than
/// closing the editor; Vim's own `v(Esc)` binding does the clearing.
#[test]
fn handle_editor_key_esc_with_an_active_vim_visual_selection_cancels_it_instead_of_closing() {
    let dir = unique_scratch_dir("editor-keymap");
    let file_path = dir.join("file.txt");
    fs::write(&file_path, "hello\n").expect("write test fixture file");
    let editor = Editor::open(file_path, None, EditorKeymapMode::Vim).expect("open test fixture file");
    let mut app = test_app(dir);
    app.mode = Mode::Editing(editor);

    handle_editor_key(&mut app, key(KeyCode::Char('v'))).unwrap(); // Vim's own "enter Visual mode"
    handle_editor_key(&mut app, key(KeyCode::Char('l'))).unwrap(); // extend the selection by one
    let Mode::Editing(editor) = &app.mode else { unreachable!() };
    assert!(editor.has_selection(), "precondition: a Visual-mode selection should be active");

    handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();

    let Mode::Editing(editor) = &app.mode else {
        panic!("Esc should cancel the selection, not close the editor");
    };
    assert!(!editor.has_selection());
    assert_eq!(editor.mode(), edtui::EditorMode::Normal);
}

/// Under Vim, `Ctrl+Shift+Right` is forwarded raw (unbound in
/// `vim_mode()`), not run through our word selection -- which forced
/// `Visual`. Nothing may change. History: docs/history/editor-keymap.md.
#[test]
fn handle_editor_key_ctrl_shift_right_is_a_noop_in_vim_mode_not_word_select() {
    let dir = unique_scratch_dir("editor-keymap");
    let file_path = dir.join("file.txt");
    fs::write(&file_path, "one two three").expect("write test fixture file");
    let editor = Editor::open(file_path, None, EditorKeymapMode::Vim).expect("open test fixture file");
    let mut app = test_app(dir);
    app.mode = Mode::Editing(editor);

    let ctrl_shift_right = KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
    handle_editor_key(&mut app, ctrl_shift_right).unwrap();

    let Mode::Editing(editor) = &app.mode else { panic!("expected Mode::Editing") };
    assert_eq!(editor.mode(), edtui::EditorMode::Normal, "must not have been forced into Visual mode");
    assert!(!editor.has_selection(), "must not have started a selection");
    assert_eq!(editor.cursor(), edtui::Index2::new(0, 0), "cursor must be untouched -- this key combination is genuinely unbound in edtui's own vim_mode()");
}

/// The same key combination must still work exactly as before under
/// `Standard` -- the fix above only needed to change `Vim`'s own
/// behavior, not regress the feature this command exists for in the
/// first place.
#[test]
fn handle_editor_key_ctrl_shift_right_still_word_selects_in_standard_mode() {
    let (mut app, _path) = open_editor_app("one two three");

    let ctrl_shift_right = KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
    handle_editor_key(&mut app, ctrl_shift_right).unwrap();

    let Mode::Editing(editor) = &app.mode else { panic!("expected Mode::Editing") };
    assert!(editor.has_selection(), "should have started a word-wise selection");
}
