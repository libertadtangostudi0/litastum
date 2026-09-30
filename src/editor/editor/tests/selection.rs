use super::*;


/// `Editor::set_keymap_mode` live-switches an already-open session:
/// rebuilds the event handler, resets `state.mode` to the new keymap's
/// own starting point, and drops any active selection (neither
/// keymap's own selection semantics carry over meaningfully to the
/// other -- `set_keymap_mode`'s own doc comment).
#[test]
fn set_keymap_mode_switches_the_handler_mode_and_drops_any_selection() {
    let (mut editor, _path) = open_test_editor("hello\n");
    editor.input(shift_key(KeyCode::Right)); // opens a selection under Standard
    assert!(editor.has_selection(), "precondition: a selection should be active");

    editor.set_keymap_mode(EditorKeymapMode::Vim);

    assert_eq!(editor.keymap_mode(), EditorKeymapMode::Vim);
    assert_eq!(editor.state.mode, EditorMode::Normal);
    assert!(!editor.has_selection());

    editor.set_keymap_mode(EditorKeymapMode::Standard);

    assert_eq!(editor.keymap_mode(), EditorKeymapMode::Standard);
    assert_eq!(editor.state.mode, EditorMode::Insert);
}


/// Integration-level regression (docs/history/shift-select.md): one `Shift+Right` right before "Draft" must select
/// exactly "D", not "Dr". Goes through the real `Editor::input`
/// (not just the raw table in `bindings/mod.rs`'s own tests), since
/// this is exactly where the interaction with
/// `wrap_line_boundary_arrow_movement` matters -- a multi-line file
/// is used specifically so a wrongly-still-firing wrap check would
/// have jumped the selection down into the next line instead of
/// stopping on "D".
#[test]
fn shift_right_selects_exactly_one_character_mid_buffer() {
    let (mut editor, _path) = open_test_editor("Draft architecture\nsecond line");

    editor.input(shift_key(KeyCode::Right));

    let selection = editor.state.selection.expect("should have started a selection");
    assert_eq!(selection.start, selection.end, "exactly one cell should be selected");
    assert_eq!(editor.state.cursor, Index2 { row: 0, col: 0 }, "should not have wrapped to the next line or moved past 'D'");
}


/// Real, integration-level regression test for the retest that
/// caught `Shift+Left` selecting the character to the *right* of
/// the cursor instead of the left. Goes through the real
/// `Editor::input` for the same reason as the `Shift+Right` test
/// above -- confirms the wrap check is correctly skipped here too
/// (the anchor-drag already fully resolves this press).
#[test]
fn shift_left_selects_exactly_one_character_mid_buffer() {
    let (mut editor, _path) = open_test_editor("Draft architecture\nsecond line");
    editor.state.cursor.col = 6; // right before the 'a' of "architecture"

    editor.input(shift_key(KeyCode::Left));

    let selection = editor.state.selection.expect("should have started a selection");
    assert_eq!(selection.start, selection.end, "exactly one cell should be selected");
    assert_eq!(editor.state.cursor, Index2 { row: 0, col: 5 }, "should be on the space right before \"architecture\", not on 'a'");
}


/// Real, integration-level regression test for the reported bug
/// (`docs/history/word-select.md`, "Fourteenth", has the full story): a selection built
/// purely by walking backward (`Ctrl+Shift+Left` twice) must retrace
/// correctly when a `Ctrl+Shift+Right` follows -- undoing exactly the
/// most recent `Left`, not blindly extending forward from wherever the
/// cursor currently sits. Goes through the real `Editor::extend_word_selection`
/// (not the raw `bindings::extend_word_selection` directly), since this
/// is exactly where the bug lived: `WordSelectTouch` tracking never
/// recognized a forward press against a `NativeBackward`-built
/// selection as a retraction.
#[test]
fn ctrl_shift_right_retraces_a_backward_built_selection() {
    let (mut editor, _path) = open_test_editor("Draft architecture derived");
    editor.state.cursor.col = 18; // the space right after "architecture"

    editor.extend_word_selection(false); // Ctrl+Shift+Left, selects "architecture"
    editor.extend_word_selection(false); // Ctrl+Shift+Left, extends through "Draft" too
    editor.extend_word_selection(true); // Ctrl+Shift+Right, should undo the second press

    let selection = editor.state.selection.expect("should still have a selection");
    assert_eq!(selection.start.col, 17, "anchor should be back on 'e', the last letter of \"architecture\"");
    assert_eq!(selection.end.col, 6, "cursor should be back on 'a', the start of \"architecture\"");
    assert_eq!(editor.state.cursor.col, 6);
}


/// Continuing past a full retrace should close the selection entirely,
/// not leave a stray one-character selection sitting on the anchor --
/// see `retreat_forward_through_a_backward_walk`'s own doc comment for
/// why the crossing check has to be `>=`, not `==`, against the anchor.
#[test]
fn ctrl_shift_right_closes_the_selection_once_the_backward_walk_is_fully_retraced() {
    let (mut editor, _path) = open_test_editor("Draft architecture derived");
    editor.state.cursor.col = 18; // the space right after "architecture"

    editor.extend_word_selection(false); // "architecture"
    editor.extend_word_selection(false); // "Draft architecture"
    editor.extend_word_selection(true); // undoes "Draft", back to "architecture"
    editor.extend_word_selection(true); // undoes "architecture" too -- nothing left

    assert_eq!(editor.state.mode, EditorMode::Insert, "should have closed the selection entirely");
    assert!(editor.state.selection.is_none());
}


/// Real, integration-level regression test for the reported bug
/// (`docs/history/word-select.md`, "Sixteenth", has the full story): retracing a backward
/// selection must return the cursor to the exact column it started
/// at, not the trimmed anchor. Goes through the real `Editor::
/// extend_word_selection`, confirming `word_select_true_anchor` is
/// actually threaded through correctly end to end, not just in the
/// lower-level `bindings::extend_word_selection` unit tests.
#[test]
fn ctrl_shift_right_after_left_returns_to_the_true_starting_column() {
    let (mut editor, _path) = open_test_editor("derived");
    editor.state.cursor.col = 4; // between 'i' and 'v'

    editor.extend_word_selection(false); // Ctrl+Shift+Left, selects "deri"
    editor.extend_word_selection(true); // Ctrl+Shift+Right, should retrace back to column 4

    assert_eq!(editor.state.mode, EditorMode::Insert, "should have closed the selection entirely");
    assert!(editor.state.selection.is_none());
    assert_eq!(editor.state.cursor.col, 4, "should be back at the exact original column, not the trimmed anchor (3)");

    // One further Right starts a fresh forward selection from there,
    // matching VS Code's own "reflect" behavior for this same case.
    editor.extend_word_selection(true);
    let selection = editor.state.selection.expect("should have started a fresh forward selection");
    assert_eq!(selection.start.col, 4);
    assert_eq!(selection.end.col, 6, "should land on the last letter of \"derived\", selecting \"ved\"");
}


/// The line-boundary-crossing edge case: `Shift+Left` right at the
/// very start of a (non-first) line has nothing to select on that
/// line at all -- must still hand off to the wrap check and cross
/// into the previous line, the same way it already does for
/// `Shift+Right` at a line's own end
/// (`shift_right_selection_extends_across_the_line_boundary` in
/// `bindings/line_wrap.rs`), rather than silently doing nothing.
#[test]
fn shift_left_at_the_start_of_a_line_still_wraps_to_the_previous_line() {
    let (mut editor, _path) = open_test_editor("hi\nworld");
    editor.state.cursor = Index2 { row: 1, col: 0 };

    editor.input(shift_key(KeyCode::Left));

    assert_eq!(editor.state.cursor.row, 0, "should have crossed into the previous line");
}


/// Real reported bug: select-all (`Ctrl+A`) then typing a character
/// left the selection completely untouched and the character never
/// appeared at all. Root cause: a plain `Char` had no binding at all
/// for `Visual` mode in `standard_key_handler`'s own table, and
/// `edtui`'s own built-in "typing inserts" fallback only fires in
/// `Insert` mode -- so the keystroke reached neither path.
#[test]
fn typing_over_a_select_all_selection_replaces_it() {
    let (mut editor, _path) = open_test_editor("hello world");
    editor.select_all();
    assert!(editor.has_selection(), "precondition");

    editor.input(key(KeyCode::Char('X')));

    assert_eq!(editor.state.lines, Lines::from("X"));
    assert!(!editor.has_selection());
    assert_eq!(editor.state.mode, EditorMode::Insert);
}


/// Same fix, for an ordinary `Shift+Right`-built selection -- not just
/// select-all -- since the underlying gap (no `Visual`-mode binding for
/// a plain `Char`) affects any active selection the same way.
#[test]
fn typing_over_a_shift_selection_replaces_it() {
    let (mut editor, _path) = open_test_editor("hello");
    editor.input(shift_key(KeyCode::Right));
    editor.input(shift_key(KeyCode::Right));
    assert!(editor.has_selection(), "precondition: \"he\" should be selected");

    editor.input(key(KeyCode::Char('X')));

    assert_eq!(editor.state.lines, Lines::from("Xllo"));
    assert!(!editor.has_selection());
}
