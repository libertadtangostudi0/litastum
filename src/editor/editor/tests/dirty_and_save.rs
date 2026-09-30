use super::*;


#[test]
fn typing_marks_dirty() {
    let (mut editor, _path) = open_test_editor("hello\n");
    editor.input(key(KeyCode::Char('!')));
    assert!(editor.is_dirty());
}


#[test]
fn save_writes_file_and_clears_dirty() {
    let (mut editor, path) = open_test_editor("hi\n");
    editor.input(key(KeyCode::Char('!')));
    editor.save().unwrap();

    assert!(!editor.is_dirty());
    assert_eq!(fs::read_to_string(&path).unwrap(), "!hi\n");
}


/// Regression test for the perf fix in `Editor::dirty`'s own doc
/// comment: navigation keys (`can_mutate_buffer` returns `false` for
/// them) must skip recomputing `dirty`, but the *cached* value from
/// before the navigation still has to come through correctly in both
/// directions -- a clean file must stay reported clean while merely
/// moving the cursor around, and a dirty one must stay reported dirty,
/// not accidentally reset by the skip.
#[test]
fn navigation_keys_never_change_the_cached_dirty_state() {
    let (mut editor, _path) = open_test_editor("hello world\nsecond line");
    assert!(!editor.is_dirty());

    for _ in 0..5 {
        editor.input(key(KeyCode::Right));
    }
    editor.input(key(KeyCode::Down));
    editor.input(key(KeyCode::Home));
    editor.input(key(KeyCode::End));
    assert!(!editor.is_dirty(), "pure navigation must not mark a clean file dirty");

    editor.input(key(KeyCode::Char('!')));
    assert!(editor.is_dirty());

    for _ in 0..5 {
        editor.input(key(KeyCode::Left));
    }
    assert!(editor.is_dirty(), "pure navigation must not clear a genuinely dirty file's flag");
}


/// Regression test for a real gap found by hand while testing Vim mode
/// against the same kind of file the fix above exists for: Vim's own
/// `h`/`j`/`k`/`l` navigation (not arrows) must get the identical
/// dirty-caching exemption, or a Vim user navigating a pathologically
/// long line with `hjkl` gets none of the perf fix's benefit at all
/// (`can_mutate_buffer`'s own doc comment has the full story, including
/// why `w`/`b`/`e`/etc. are deliberately *not* also exempted).
#[test]
fn vim_hjkl_navigation_never_changes_the_cached_dirty_state() {
    let path = unique_scratch_dir("editor").join("file.txt");
    fs::write(&path, "hello world\nsecond line").expect("write test fixture file");
    let mut editor = Editor::open(path, None, EditorKeymapMode::Vim).expect("open test fixture file");
    assert!(!editor.is_dirty());

    for _ in 0..5 {
        editor.input(key(KeyCode::Char('l')));
    }
    editor.input(key(KeyCode::Char('j')));
    editor.input(key(KeyCode::Char('h')));
    editor.input(key(KeyCode::Char('k')));
    assert!(!editor.is_dirty(), "pure hjkl navigation must not mark a clean file dirty");

    editor.input(key(KeyCode::Char('x'))); // deletes the character under the cursor
    assert!(editor.is_dirty(), "a genuine Vim edit must still be detected");

    for _ in 0..5 {
        editor.input(key(KeyCode::Char('l')));
    }
    assert!(editor.is_dirty(), "further hjkl navigation must not clear a genuinely dirty file's flag");
}


/// The exemption above must be Vim-specific -- under `Standard`, 'h'/
/// 'j'/'k'/'l' are ordinary letters that insert text, and must still be
/// correctly detected as edits (this pins down that the fix didn't
/// accidentally make the exemption keymap-independent).
#[test]
fn hjkl_still_marks_standard_mode_dirty_as_ordinary_typed_characters() {
    let (mut editor, _path) = open_test_editor("hello\n");
    assert!(!editor.is_dirty());

    for c in ['h', 'j', 'k', 'l'] {
        editor.input(key(KeyCode::Char(c)));
    }

    assert!(editor.is_dirty(), "hjkl typed in Standard mode are literal characters, not navigation");
}
