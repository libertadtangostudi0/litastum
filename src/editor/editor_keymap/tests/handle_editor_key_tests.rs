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

/// Regression test for the real, reported bug: `Ctrl+A` then
/// `Backspace` then a single `Ctrl+Z` did nothing at all -- only a
/// *second* `Ctrl+Z` actually restored the deleted text. Root cause:
/// the old table entry captured an undo checkpoint twice for one
/// keypress (once correctly, inside `DeleteSelection`, and once more
/// spuriously when returning to `Insert` mode afterward) -- see
/// `bindings::is_selection_consuming_key`'s own doc comment for the
/// full mechanism. One `Ctrl+Z` must restore everything now.
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

/// Same as `handle_editor_key_esc_with_an_active_selection_cancels_the_selection_instead_of_closing`,
/// but through a real Vim `Visual`-mode selection (opened with `v`,
/// Vim's own binding, not `Shift+Right`) -- confirms `Close`'s own
/// `editor.has_selection()` check and the subsequent forward-to-`input`
/// both work correctly against `edtui`'s own Vim state too, not just
/// `Standard`'s. `Esc` reaching `Editor::input` there hits Vim's own
/// `v(Esc) -> SwitchMode(Normal)` binding, which is what actually
/// clears the selection here -- not any of this project's own logic.
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

/// Regression test for a real bug found by hand while testing Vim
/// mode directly: `Ctrl+Shift+Right` used to run
/// `Editor::extend_word_selection` (this project's own hand-rolled,
/// Standard-keymap-tuned word-selection logic) regardless of
/// `keymap_mode`, forcing `state.mode` into `Visual` completely
/// outside any of Vim's own bindings -- contradicting
/// `EditorKeymapMode::Vim`'s own documented promise that none of
/// this project's correction passes run while Vim is active.
/// `edtui`'s own `vim_mode()` table has no entry for this key
/// combination either, so the fix (forwarding the raw key instead)
/// must leave the editor completely unaffected -- no selection, no
/// mode change, cursor untouched.
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
