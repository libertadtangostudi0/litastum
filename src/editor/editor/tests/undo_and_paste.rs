use super::*;


#[test]
fn undo_after_save_makes_it_dirty_again() {
    // `dirty` is recomputed by comparing against the saved snapshot on
    // every key that could plausibly have mutated the buffer
    // (`can_mutate_buffer`) -- Ctrl+Z is one of those, so this should
    // "just work" even though `dirty` is now a cached field rather than
    // a fresh comparison on every `is_dirty()` call (see `Editor::dirty`'s
    // own doc comment for why that changed).
    let (mut editor, _path) = open_test_editor("hi\n");
    editor.input(key(KeyCode::Char('!')));
    editor.save().unwrap();
    assert!(!editor.is_dirty());

    editor.input(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
    assert!(editor.is_dirty(), "undoing past the saved state should be dirty again");
}


/// Regression coverage for the real request: `Ctrl+Z` right after a
/// fast paste (`fast_paste_from_clipboard`) should undo exactly that
/// paste in one press -- seeded directly (`editor.undo_stack`,
/// `editor.state.lines`) rather than through a real `Ctrl+V`, since
/// that would mean touching the real OS clipboard in a test (this
/// codebase deliberately avoids that elsewhere too).
#[test]
fn ctrl_z_restores_the_fast_paste_snapshot_in_one_press() {
    let (mut editor, _path) = open_test_editor("hello\n");
    let pre_paste_lines = editor.state.lines.clone();
    let pre_paste_cursor = editor.state.cursor;

    // Simulate what fast_paste_from_clipboard itself would have just
    // done: save the snapshot, then mutate the live buffer.
    editor.undo_stack.push(Snapshot { lines: pre_paste_lines.clone(), cursor: pre_paste_cursor });
    editor.state.lines = Lines::from("hello pasted stuff\n");
    editor.state.cursor = Index2::new(0, 18);
    editor.dirty = true;

    editor.input(ctrl_key('z'));

    assert_eq!(editor.state.lines, pre_paste_lines, "the buffer should be back to exactly its pre-paste content");
    assert_eq!(editor.state.cursor, pre_paste_cursor);
    assert!(editor.undo_stack.is_empty(), "the consumed snapshot should be popped, not reusable by a second Ctrl+Z");
}


/// Real reported bug: two (or more) consecutive fast pastes only ever
/// gave back one `Ctrl+Z` of undo -- the second press fell straight
/// through to `edtui`'s own (empty, since fast pastes never touch it)
/// undo stack and did nothing, silently leaving the *first* paste's
/// own content stuck in the buffer. `paste_text` pushes a fresh
/// snapshot per call rather than overwriting a single slot, so two
/// consecutive pastes must undo one at a time, most recent first.
#[test]
fn consecutive_fast_pastes_undo_one_at_a_time_most_recent_first() {
    let (mut editor, _path) = open_test_editor("");
    let original_lines = editor.state.lines.clone();

    editor.paste_text("first ");
    let after_first_paste = editor.state.lines.clone();
    // paste_text lands the cursor *on* the last pasted character (the
    // trailing space here), matching PasteBefore's own vim-`P`
    // convention -- nudge forward to the append position first, same as
    // a real subsequent `Ctrl+V` would land after a user's own cursor
    // move, so this test's own two pastes don't collide mid-word.
    editor.set_cursor(Index2::new(0, editor.state.lines.len_col(0).unwrap()));
    editor.paste_text("second");
    assert_eq!(editor.state.lines, Lines::from("first second"), "sanity: both pastes landed");

    editor.input(ctrl_key('z'));
    assert_eq!(editor.state.lines, after_first_paste, "first Ctrl+Z should undo only the second paste");

    editor.input(ctrl_key('z'));
    assert_eq!(editor.state.lines, original_lines, "second Ctrl+Z should undo the first paste too, back to the original content");
}


/// `Limits::max_paste_undo_stack` (default 20, hardcoded in a test
/// build per `theming::config::limits`'s own test-isolation rule) caps
/// the stack, dropping the *oldest* entry once exceeded -- guards
/// against an unbounded number of full-buffer clones piling up in
/// memory from many consecutive pastes with nothing else in between
/// (see that field's own doc comment for why this matters given this
/// project's own stated large-file scale target).
#[test]
fn paste_undo_stack_is_capped_dropping_the_oldest_entry() {
    let (mut editor, _path) = open_test_editor("");
    let cap = crate::theming::config::limits().max_paste_undo_stack;

    for i in 0..cap + 3 {
        editor.paste_text(&format!("{i} "));
    }

    assert_eq!(editor.undo_stack.len(), cap, "the stack must never grow past the configured cap");
}


/// The actual reported bug, end to end: paste, then do something else
/// (typing further characters), then undo repeatedly. The later typed
/// characters must undo one at a time first (ordinary, expected
/// granularity), and once undo reaches back to the paste itself, the
/// *whole* pasted block must disappear in one further `Ctrl+Z` -- not
/// character by character, which is what happened before `Editor`
/// owned its own full undo stack (the paste had no boundary `edtui`'s
/// own undo could ever see, once a later edit's own checkpoint became
/// the closest thing behind it).
#[test]
fn undo_treats_a_paste_as_one_block_even_after_later_edits() {
    let (mut editor, _path) = open_test_editor("");

    editor.paste_text("pasted block");
    let after_paste = editor.state.lines.clone();
    editor.set_cursor(Index2::new(0, editor.state.lines.len_col(0).unwrap()));

    editor.input(key(KeyCode::Char('a')));
    editor.input(key(KeyCode::Char('b')));
    assert_eq!(editor.state.lines, Lines::from("pasted blockab"), "sanity");

    editor.input(ctrl_key('z'));
    editor.input(ctrl_key('z'));
    assert_eq!(editor.state.lines, after_paste, "the two typed characters should undo individually, first");

    editor.input(ctrl_key('z'));
    assert_eq!(editor.state.lines, Lines::from(""), "the entire pasted block should disappear in this one further Ctrl+Z, not character by character");
}


#[test]
fn ctrl_y_redoes_what_ctrl_z_just_undid() {
    let (mut editor, _path) = open_test_editor("hello");
    editor.set_cursor(Index2::new(0, 5)); // end of "hello"

    editor.input(key(KeyCode::Char('!')));
    assert_eq!(editor.state.lines, Lines::from("hello!"));

    editor.input(ctrl_key('z'));
    assert_eq!(editor.state.lines, Lines::from("hello"));

    editor.input(ctrl_key('y'));
    assert_eq!(editor.state.lines, Lines::from("hello!"));
}


/// Standard editor convention: a new edit made after undoing should
/// clear whatever redo history came before it -- redoing shouldn't be
/// able to resurrect a branch of history a fresh edit already diverged
/// away from.
#[test]
fn a_new_edit_after_undo_clears_redo_history() {
    let (mut editor, _path) = open_test_editor("hello");
    editor.set_cursor(Index2::new(0, 5)); // end of "hello"
    editor.input(key(KeyCode::Char('!')));
    editor.input(ctrl_key('z'));
    editor.input(key(KeyCode::Char('?')));
    assert_eq!(editor.state.lines, Lines::from("hello?"), "sanity");

    let redid = editor.redo();

    assert!(!redid, "there should be nothing left to redo after a fresh edit");
    assert_eq!(editor.state.lines, Lines::from("hello?"), "the fresh edit must not be disturbed by a no-op redo");
}


/// Real requested behavior: the paste-undo stack is invalidated
/// entirely -- once any other key happens, `Ctrl+Z` must fall through
/// to `edtui`'s own real undo stack instead of resurrecting a stale
/// paste snapshot.
///
/// Since `Editor` now owns its own full undo stack (not just a
/// paste-specific one -- see `input`'s own doc comment for why), a
/// pending snapshot is never blindly cleared by "any other key" the way
/// an earlier, paste-only version of this worked -- pure navigation
/// (which can never mutate the buffer, `can_mutate_buffer`) leaves an
/// existing snapshot alone, since there's nothing to invalidate; a real
/// edit pushes its *own* snapshot on top instead of discarding the
/// earlier one, so both stay undoable, most recent first.
#[test]
fn navigation_leaves_a_pending_snapshot_untouched_but_a_real_edit_pushes_its_own() {
    let (mut editor, _path) = open_test_editor("hello\n");
    editor.undo_stack.push(Snapshot { lines: editor.state.lines.clone(), cursor: editor.state.cursor });

    editor.input(key(KeyCode::Right));
    assert_eq!(editor.undo_stack.len(), 1, "pure navigation must not touch the undo stack at all");

    editor.input(key(KeyCode::Char('!')));
    assert_eq!(editor.undo_stack.len(), 2, "a real edit should push its own snapshot, not discard the earlier one");
}


/// `Ctrl+Z` with no pending fast-paste snapshot must not panic or do
/// anything paste-undo-specific -- `input` should just fall through to
/// `edtui`'s own real `Undo` action, unaffected by this feature
/// existing at all.
#[test]
fn ctrl_z_with_no_pending_paste_falls_through_to_the_real_undo_action() {
    let (mut editor, _path) = open_test_editor("hello\n");
    assert!(editor.undo_stack.is_empty(), "sanity");

    editor.input(ctrl_key('z')); // should not panic

    assert!(editor.undo_stack.is_empty());
}


/// `Editor::paste_text` (the shared core `fast_paste_from_clipboard`
/// and `event_loop::paste::handle_paste_event`'s real bracketed paste both use)
/// -- exercised directly here since it takes the text as a plain
/// argument, no real clipboard needed.
#[test]
fn paste_text_splices_and_records_an_undo_snapshot() {
    let (mut editor, _path) = open_test_editor("held\n");
    editor.state.cursor = Index2::new(0, 2);
    let pre_paste_lines = editor.state.lines.clone();

    editor.paste_text("llo wor");

    assert_eq!(editor.state.lines, Lines::from("hello world\n"));
    assert!(editor.is_dirty());
    assert!(!editor.undo_stack.is_empty(), "should have recorded an undo snapshot");

    editor.input(ctrl_key('z'));
    assert_eq!(editor.state.lines, pre_paste_lines, "Ctrl+Z should restore exactly the pre-paste content");
}


/// A bracketed paste while text is selected should clear the selection
/// and drop back to typing mode -- same simplification `Ctrl+V` itself
/// already has (see `paste_text`'s own doc comment), replicated by hand
/// here since it bypasses `edtui`'s own dispatch.
#[test]
fn paste_text_over_an_active_selection_clears_it_and_returns_to_insert_mode() {
    let (mut editor, _path) = open_test_editor("hello world");
    editor.input(shift_key(KeyCode::Right)); // opens a selection
    assert!(editor.has_selection(), "precondition");

    editor.paste_text("X");

    assert!(!editor.has_selection());
    assert_eq!(editor.state.mode, EditorMode::Insert);
}


/// Empty text is a real no-op, not just "nothing visible changes" --
/// no undo snapshot should be recorded either, or a stray `Ctrl+Z`
/// right after would undo something that never actually happened.
#[test]
fn paste_text_with_empty_text_records_no_undo_snapshot() {
    let (mut editor, _path) = open_test_editor("hello\n");

    editor.paste_text("");

    assert!(!editor.is_dirty());
    assert!(editor.undo_stack.is_empty());
}
