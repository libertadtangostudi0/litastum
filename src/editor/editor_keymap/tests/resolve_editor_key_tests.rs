use super::*;

#[test]
fn ctrl_s_resolves_to_save() {
    assert_eq!(resolve(ctrl_key('s')), EditorCommand::Save);
}

#[test]
fn ctrl_shift_s_uppercase_still_resolves_to_save() {
    assert_eq!(resolve(ctrl_key('S')), EditorCommand::Save);
}

#[test]
fn esc_resolves_to_close_even_without_ctrl() {
    let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(resolve(key), EditorCommand::Close);
}

#[test]
fn plain_s_without_ctrl_is_forwarded_not_save() {
    let key = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE);
    assert_eq!(resolve(key), EditorCommand::Forward);
}

#[test]
fn unmodified_letter_is_forwarded() {
    let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
    assert_eq!(resolve(key), EditorCommand::Forward);
}

#[test]
fn ctrl_f_resolves_to_find() {
    assert_eq!(resolve(ctrl_key('f')), EditorCommand::Find);
    assert_eq!(resolve(ctrl_key('F')), EditorCommand::Find, "should match uppercase too, same reasoning as Ctrl+S");
}

#[test]
fn f7_resolves_to_find_like_in_far() {
    assert_eq!(resolve(KeyEvent::new(KeyCode::F(7), KeyModifiers::NONE)), EditorCommand::Find);
}

#[test]
fn shift_f7_and_alt_f7_step_through_matches_like_in_far() {
    assert_eq!(resolve(KeyEvent::new(KeyCode::F(7), KeyModifiers::SHIFT)), EditorCommand::FindNext);
    assert_eq!(resolve(KeyEvent::new(KeyCode::F(7), KeyModifiers::ALT)), EditorCommand::FindPrevious);
}

#[test]
fn plain_f_without_ctrl_is_forwarded_not_find() {
    let key = KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE);
    assert_eq!(resolve(key), EditorCommand::Forward);
}

#[test]
fn ctrl_a_resolves_to_select_all() {
    assert_eq!(resolve(ctrl_key('a')), EditorCommand::SelectAll);
    assert_eq!(resolve(ctrl_key('A')), EditorCommand::SelectAll, "should match uppercase too, same reasoning as Ctrl+S");
}

#[test]
fn plain_a_without_ctrl_is_forwarded_not_select_all() {
    let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
    assert_eq!(resolve(key), EditorCommand::Forward);
}

#[test]
fn ctrl_c_is_forwarded_to_edtui_not_handled_here() {
    // Copy/cut/paste are edtui's own concern now (see its custom
    // keymap in editor.rs) -- this module no longer special-cases them.
    assert_eq!(resolve(ctrl_key('c')), EditorCommand::Forward);
}

#[test]
fn ctrl_shift_right_resolves_to_word_select_forward() {
    let key = KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
    assert_eq!(resolve(key), EditorCommand::WordSelect { forward: true });
}

#[test]
fn ctrl_shift_left_resolves_to_word_select_backward() {
    let key = KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
    assert_eq!(resolve(key), EditorCommand::WordSelect { forward: false });
}

#[test]
fn plain_ctrl_right_without_shift_is_forwarded_to_edtui() {
    // Plain Ctrl+Right (no selection) is still `bindings.rs`'s own
    // declarative-table concern -- only the Shift combination is
    // special-cased here.
    let key = KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL);
    assert_eq!(resolve(key), EditorCommand::Forward);
}

#[test]
fn plain_shift_right_without_ctrl_is_forwarded_to_edtui() {
    // Character-wise Shift+Right stays edtui's own table entry too.
    let key = KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT);
    assert_eq!(resolve(key), EditorCommand::Forward);
}

/// Regression test for the real crash: `F10` (the app's own global
/// quit key on the browsing screen) has no conversion in `edtui`'s
/// own `KeyCode::from` at all -- forwarding it panicked the whole
/// process. Must resolve to `Ignore`, not `Forward`.
#[test]
fn f10_is_ignored_not_forwarded() {
    let key = KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE);
    assert_eq!(resolve(key), EditorCommand::Ignore);
}

/// Every function key shares the same gap in `edtui`'s own
/// conversion, not just `F10` -- pinned down as a range rather than
/// one magic number.
#[test]
fn every_function_key_is_ignored_not_forwarded() {
    for n in 1..=12 {
        if n == 9 || n == 3 || n == 7 || n == 2 {
            continue; // F9 opens the editor's own settings menu, F3 is next search match, F7 opens search, F2 saves -- see their own tests.
        }
        let key = KeyEvent::new(KeyCode::F(n), KeyModifiers::NONE);
        assert_eq!(resolve(key), EditorCommand::Ignore, "F{n} should be ignored, not forwarded to edtui");
    }
}

#[test]
fn f2_saves_and_shift_f2_saves_as() {
    assert_eq!(resolve(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE)), EditorCommand::Save);
    assert_eq!(resolve(KeyEvent::new(KeyCode::F(2), KeyModifiers::SHIFT)), EditorCommand::SaveAs);
}

#[test]
fn f3_and_shift_f3_step_through_search_matches() {
    assert_eq!(resolve(KeyEvent::new(KeyCode::F(3), KeyModifiers::NONE)), EditorCommand::FindNext);
    assert_eq!(resolve(KeyEvent::new(KeyCode::F(3), KeyModifiers::SHIFT)), EditorCommand::FindPrevious);
}

#[test]
fn f9_opens_the_editor_menu() {
    let key = KeyEvent::new(KeyCode::F(9), KeyModifiers::NONE);
    assert_eq!(resolve(key), EditorCommand::OpenMenu);
}

/// `Insert` is a real crossterm `KeyCode` variant `edtui`'s own
/// conversion also has no arm for -- confirms this isn't
/// function-keys-only special-casing.
#[test]
fn insert_key_is_ignored_not_forwarded() {
    let key = KeyEvent::new(KeyCode::Insert, KeyModifiers::NONE);
    assert_eq!(resolve(key), EditorCommand::Ignore);
}
