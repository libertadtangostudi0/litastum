use super::*;

#[test]
fn handle_confirm_discard_key_y_discards_and_returns_to_browsing() {
    let (mut app, _path) = open_editor_app("hi\n");
    handle_editor_key(&mut app, key(KeyCode::Char('!'))).unwrap();
    handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();
    assert!(matches!(app.overlay, Some(Overlay::ConfirmDiscard)));
    assert!(matches!(app.mode, Mode::Editing(_)), "the editor stays open under the prompt");

    handle_confirm_discard_key(&mut app, key(KeyCode::Char('y'))).unwrap();

    assert!(matches!(app.mode, Mode::Browsing));
}

#[test]
fn handle_confirm_discard_key_n_cancels_back_into_the_editor_with_changes_intact() {
    let (mut app, _path) = open_editor_app("hi\n");
    handle_editor_key(&mut app, key(KeyCode::Char('!'))).unwrap();
    handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();

    handle_confirm_discard_key(&mut app, key(KeyCode::Char('n'))).unwrap();

    let Mode::Editing(editor) = &app.mode else {
        panic!("Cancel should return to Mode::Editing, not discard");
    };
    assert!(editor.is_dirty(), "the unsaved change should still be there");
}

#[test]
fn handle_confirm_discard_key_ignores_unrelated_keys_and_stays_open() {
    let (mut app, _path) = open_editor_app("hi\n");
    handle_editor_key(&mut app, key(KeyCode::Char('!'))).unwrap();
    handle_editor_key(&mut app, key(KeyCode::Esc)).unwrap();

    handle_confirm_discard_key(&mut app, key(KeyCode::Char('x'))).unwrap();

    assert!(matches!(app.overlay, Some(Overlay::ConfirmDiscard)));
    assert!(matches!(app.mode, Mode::Editing(_)), "the editor stays open under the prompt");
}
