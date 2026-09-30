use edtui::clipboard::InternalClipboard;
use edtui::{EditorEventHandler, EditorMode, EditorState, Index2, Lines};

use super::*;
use crate::test_support::{ctrl_code_key, ctrl_key, key, shift_key};

/// Builds an `EditorState` + our custom keymap directly (bypassing
/// `Editor::open`'s real-file / real-OS-clipboard setup) with
/// `InternalClipboard`, so copy/cut/paste tests never touch the
/// actual system clipboard -- that would be flaky in CI and rude to
/// whatever the developer running the tests had copied.
fn test_state(contents: &str) -> (EditorState, EditorEventHandler) {
    let mut state = EditorState::new(Lines::from(contents));
    state.mode = EditorMode::Insert;
    state.set_clipboard(InternalClipboard::default());
    (state, EditorEventHandler::new(standard_key_handler()))
}

/// One key through the real table plus the correction passes, in
/// `Editor::input`'s order -- the fresh `Shift`+arrow tests need both
/// halves, and a first `Shift+Left` test that used the raw table alone
/// missed half of the fix. `vertical_shift_anchor_col` stands in for
/// `Editor`'s field: each test declares one and threads it through.
fn input(
    state: &mut EditorState,
    handler: &mut EditorEventHandler,
    key_event: crossterm::event::KeyEvent,
    vertical_shift_anchor_col: &mut Option<usize>,
) {
    let cursor_before = state.cursor;
    let mode_before = state.mode;
    handler.on_key_event(key_event, state);

    if mode_before == EditorMode::Visual && is_selection_consuming_key(&key_event) {
        state.selection = None;
        state.mode = EditorMode::Insert;
    }

    let freshly_entered_visual = mode_before != EditorMode::Visual && state.mode == EditorMode::Visual;
    let anchored_on_a_real_character = freshly_entered_visual && anchor_fresh_shift_selection(state, key_event.code, cursor_before);

    if !anchored_on_a_real_character {
        wrap_line_boundary_arrow_movement(state, key_event.code, key_event.modifiers, cursor_before);
    }

    if freshly_entered_visual && matches!(key_event.code, KeyCode::Up | KeyCode::Down) {
        *vertical_shift_anchor_col = Some(cursor_before.col);
        exclude_landing_column_on_fresh_vertical_selection(state, key_event.code);
    }

    close_selection_if_back_on_the_anchors_row(state, key_event.code, vertical_shift_anchor_col);
}

#[test]
fn typing_inserts_characters() {
    let (mut state, mut handler) = test_state("");
    handler.on_key_event(key(KeyCode::Char('h')), &mut state);
    handler.on_key_event(key(KeyCode::Char('i')), &mut state);
    assert_eq!(String::from(state.lines.clone()), "hi");
}

#[test]
fn shift_right_starts_a_selection() {
    let (mut state, mut handler) = test_state("hello");
    handler.on_key_event(shift_key(KeyCode::Right), &mut state);
    assert_eq!(state.mode, EditorMode::Visual);
    assert!(state.selection.is_some());
}

/// Regression test for the real report: one `Shift+Right` right
/// before "Draft" selected two characters ("Dr"), not one ("D").
/// `SwitchMode(Visual)` alone already anchors a one-character
/// selection on the current cell (vim's own `v` semantics) -- the
/// fresh table entries no longer chain a `Move` on top of that (see
/// their own comment in `standard_key_handler`), so a single press
/// should select exactly the one character the cursor started on.
#[test]
fn shift_right_selects_exactly_one_character_not_two() {
    let (mut state, mut handler) = test_state("Draft architecture");
    let mut vertical_shift_anchor_col = None;
    input(&mut state, &mut handler, shift_key(KeyCode::Right), &mut vertical_shift_anchor_col);

    {
        let selection = state.selection.as_ref().expect("should have started a selection");
        assert_eq!(selection.start, selection.end, "exactly one cell should be selected");
    }
    assert_eq!(state.cursor.col, 0, "the anchor cell itself -- 'D' -- not moved past it");

    handler.on_key_event(ctrl_key('c'), &mut state);
    handler.on_key_event(key(KeyCode::End), &mut state);
    handler.on_key_event(ctrl_key('v'), &mut state);
    assert_eq!(String::from(state.lines.clone()), "Draft architectureD");
}

/// Regression: `Shift+Left` selected (and copied) the character to the
/// *right* of the cursor. History: docs/history/shift-select.md (2).
#[test]
fn shift_left_selects_the_character_actually_to_the_left() {
    let (mut state, mut handler) = test_state("Draft architecture");
    state.cursor.col = 6; // right before the 'a' of "architecture"
    let mut vertical_shift_anchor_col = None;

    input(&mut state, &mut handler, shift_key(KeyCode::Left), &mut vertical_shift_anchor_col);

    {
        let selection = state.selection.as_ref().expect("should have started a selection");
        assert_eq!(selection.start, selection.end, "exactly one cell should be selected");
    }
    assert_eq!(state.cursor.col, 5, "should have moved onto the space right before \"architecture\" -- the character actually to the left");

    handler.on_key_event(ctrl_key('c'), &mut state);
    handler.on_key_event(key(KeyCode::End), &mut state);
    handler.on_key_event(ctrl_key('v'), &mut state);
    assert_eq!(String::from(state.lines.clone()), "Draft architecture ", "should have copied the space, not 'a'");
}

/// Regression (aligned text): a fresh `Shift+Down` lands one column
/// short of the aligned destination column, so the destination row's
/// copy of the word there isn't selected. History:
/// docs/history/shift-select.md (4, 5).
#[test]
fn shift_down_lands_one_column_short_of_the_aligned_destination() {
    let (mut state, mut handler) = test_state("line one\nline two");
    state.cursor.col = 2; // the 'n' of "line", first line
    let mut vertical_shift_anchor_col = None;

    input(&mut state, &mut handler, shift_key(KeyCode::Down), &mut vertical_shift_anchor_col);

    assert_eq!(state.cursor, Index2 { row: 1, col: 1 }, "should have trimmed the aligned landing column by one");
}

/// A second `Shift+Down` press keeps descending one line at a time from
/// wherever the first press's trim left the column -- the trim only
/// ever applies once, on the fresh press (`MoveDown` never touches
/// `.col` on its own, so there's nothing further to trim on a
/// continuing press).
#[test]
fn repeated_shift_down_keeps_the_trimmed_column() {
    let (mut state, mut handler) = test_state("one\ntwo\nthree");
    state.cursor.col = 1;
    let mut vertical_shift_anchor_col = None;

    input(&mut state, &mut handler, shift_key(KeyCode::Down), &mut vertical_shift_anchor_col);
    input(&mut state, &mut handler, shift_key(KeyCode::Down), &mut vertical_shift_anchor_col);

    assert_eq!(state.cursor, Index2 { row: 2, col: 0 }, "column 1 trimmed to 0 on the first press, unchanged on the second");
}

/// `Shift+Down` then `Shift+Up` (or the reverse) returns to exactly the
/// start with nothing selected -- the trimmed column is restored from
/// `vertical_shift_anchor_col`. The report left `"te"` selected. History: docs/history/shift-select.md.
#[test]
fn shift_down_then_shift_up_returns_to_an_empty_selection_at_the_start() {
    let (mut state, mut handler) = test_state("terminal one\nterminal two");
    state.cursor.col = 4; // between the 't' and 'e' of "terminal"
    let start = state.cursor;
    let mut vertical_shift_anchor_col = None;

    input(&mut state, &mut handler, shift_key(KeyCode::Down), &mut vertical_shift_anchor_col);
    input(&mut state, &mut handler, shift_key(KeyCode::Up), &mut vertical_shift_anchor_col);

    assert_eq!(state.cursor, start, "should be back exactly where it started");
    assert_eq!(state.mode, EditorMode::Insert, "should have closed the selection entirely, not left one character selected");
    assert!(state.selection.is_none());
}

/// Same round trip, the other order: `Shift+Up` then `Shift+Down`.
#[test]
fn shift_up_then_shift_down_returns_to_an_empty_selection_at_the_start() {
    let (mut state, mut handler) = test_state("terminal one\nterminal two");
    state.cursor.row = 1;
    state.cursor.col = 4;
    let start = state.cursor;
    let mut vertical_shift_anchor_col = None;

    input(&mut state, &mut handler, shift_key(KeyCode::Up), &mut vertical_shift_anchor_col);
    input(&mut state, &mut handler, shift_key(KeyCode::Down), &mut vertical_shift_anchor_col);

    assert_eq!(state.cursor, start, "should be back exactly where it started");
    assert_eq!(state.mode, EditorMode::Insert, "should have closed the selection entirely, not left one character selected");
    assert!(state.selection.is_none());
}

#[test]
fn select_copy_paste_roundtrip() {
    let (mut state, mut handler) = test_state("hello world");

    // Each Shift+Right now selects exactly one more character than
    // the last (the fresh-selection fix above) -- 5 presses for
    // "hello"'s own 5 characters, not 4 (the old "N+1, not N"
    // quirk this fix removed -- see shift_select.rs).
    let mut vertical_shift_anchor_col = None;
    for _ in 0..5 {
        handler.on_key_event(shift_key(KeyCode::Right), &mut state); // select "hello"
    }
    input(&mut state, &mut handler, ctrl_key('c'), &mut vertical_shift_anchor_col);
    assert_eq!(state.mode, EditorMode::Insert, "copy should return to typing mode");
    assert!(state.selection.is_none());

    handler.on_key_event(key(KeyCode::End), &mut state);
    handler.on_key_event(ctrl_key('v'), &mut state);

    assert_eq!(String::from(state.lines.clone()), "hello worldhello");
}

/// Copy/paste after a word-wise retraction ("Eighth"), asserted on the
/// clipboard text rather than coordinates -- the reported copy bug turned
/// out to be the selection landing wrong, but copying is pinned on its own.
#[test]
fn word_select_copy_paste_roundtrip() {
    let (mut state, mut handler) = test_state("hello world wide web");

    extend_word_selection(&mut state, true, false, &mut None); // Ctrl+Shift+Right, selects "hello"
    extend_word_selection(&mut state, true, false, &mut None); // Ctrl+Shift+Right, extends through " world"
    extend_word_selection(&mut state, false, true, &mut None); // Ctrl+Shift+Left, retracts back onto "hello"

    let mut vertical_shift_anchor_col = None;
    input(&mut state, &mut handler, ctrl_key('c'), &mut vertical_shift_anchor_col);
    assert_eq!(state.mode, EditorMode::Insert, "copy should return to typing mode");
    assert!(state.selection.is_none());

    handler.on_key_event(key(KeyCode::End), &mut state);
    handler.on_key_event(ctrl_key('v'), &mut state);

    assert_eq!(
        String::from(state.lines.clone()),
        "hello world wide webhello ",
        "pasted text should be exactly what the word-select landed on -- \"hello \" (the word plus its \
         own trailing space, per the retraction fix), nothing extra and nothing missing"
    );
}

/// "Ninth": a whole line selected character-wise, then one
/// `Ctrl+Shift+Left` must stop on the `-` of "arrow-key", not on `k`.
/// Checked via the pasted-back text. History: docs/history/word-select.md.
#[test]
fn word_select_retraction_across_punctuation_matches_what_gets_copied() {
    let text = "theme, 2-column panels, arrow-key";
    let (mut state, mut handler) = test_state(text);
    // One Shift+Right per character now (the fresh-selection fix in
    // shift_select.rs), not `len - 1` -- see select_copy_paste_roundtrip.
    for _ in 0..text.chars().count() {
        handler.on_key_event(shift_key(KeyCode::Right), &mut state); // select the whole line, character-wise -- not word-select
    }
    assert_eq!(state.selection.as_ref().unwrap().end, state.cursor, "should have selected all the way to the last real character");

    extend_word_selection(&mut state, false, true, &mut None); // Ctrl+Shift+Left; retracting=true, as `Editor` computes for an untouched selection

    handler.on_key_event(ctrl_key('c'), &mut state);
    handler.on_key_event(key(KeyCode::End), &mut state);
    handler.on_key_event(ctrl_key('v'), &mut state);

    assert_eq!(
        String::from(state.lines.clone()),
        "theme, 2-column panels, arrow-keytheme, 2-column panels, arrow-",
        "one Ctrl+Shift+Left on a fully-selected line should retract the whole trailing word AND land \
         on the separator in front of it, even when that separator is punctuation ('-') rather than \
         whitespace -- and the copied text must match exactly what was selected"
    );
}

/// "Fifteenth": retracting past a mid-buffer anchor closes the selection;
/// pasting afterwards inserts nothing. History: docs/history/word-select.md.
#[test]
fn word_select_copy_after_retracting_past_the_anchor_copies_nothing() {
    let (mut state, mut handler) = test_state("Draft architecture derived");
    state.cursor.col = 6; // right before the 'a' of "architecture"

    extend_word_selection(&mut state, true, false, &mut None); // "architecture"
    extend_word_selection(&mut state, true, false, &mut None); // "architecture derived"
    extend_word_selection(&mut state, false, true, &mut None); // retract "derived"
    extend_word_selection(&mut state, false, true, &mut None); // retract "architecture" -- closes entirely

    handler.on_key_event(ctrl_key('c'), &mut state);
    handler.on_key_event(key(KeyCode::End), &mut state);
    handler.on_key_event(ctrl_key('v'), &mut state);

    assert_eq!(
        String::from(state.lines.clone()),
        "Draft architecture derived",
        "nothing was selected at the moment of Ctrl+C -- paste should be a no-op"
    );
}

/// One further `Ctrl+Shift+Left` past the closing above starts an
/// entirely ordinary fresh backward selection from the same anchor
/// position, landing on `"Draft "` in full -- confirmed here at the
/// clipboard level too, matching `word_select::tests::
/// ctrl_shift_left_one_more_press_past_the_closing_selects_the_previous_word`'s
/// own coordinate-level assertion.
#[test]
fn word_select_copy_one_more_press_past_the_closing_selects_the_previous_word() {
    let (mut state, mut handler) = test_state("Draft architecture derived");
    state.cursor.col = 6; // right before the 'a' of "architecture"

    extend_word_selection(&mut state, true, false, &mut None); // "architecture"
    extend_word_selection(&mut state, true, false, &mut None); // "architecture derived"
    extend_word_selection(&mut state, false, true, &mut None); // retract "derived"
    extend_word_selection(&mut state, false, true, &mut None); // retract "architecture" -- closes entirely
    extend_word_selection(&mut state, false, true, &mut None); // one more Left -- a fresh press now, selects "Draft "

    handler.on_key_event(ctrl_key('c'), &mut state);
    handler.on_key_event(key(KeyCode::End), &mut state);
    handler.on_key_event(ctrl_key('v'), &mut state);

    assert_eq!(
        String::from(state.lines.clone()),
        "Draft architecture derivedDraft ",
        "should have copied exactly \"Draft \" (the word plus its own trailing space)"
    );
}

#[test]
fn ctrl_x_cuts_the_selection() {
    let (mut state, mut handler) = test_state("hello world");

    let mut vertical_shift_anchor_col = None;
    for _ in 0..6 {
        handler.on_key_event(shift_key(KeyCode::Right), &mut state); // select "hello " -- 6 presses for 6 characters now, see select_copy_paste_roundtrip
    }
    input(&mut state, &mut handler, ctrl_key('x'), &mut vertical_shift_anchor_col);

    assert_eq!(String::from(state.lines.clone()), "world");
    assert_eq!(state.mode, EditorMode::Insert);

    handler.on_key_event(ctrl_key('v'), &mut state);
    assert_eq!(String::from(state.lines.clone()), "hello world");
}

#[test]
fn esc_cancels_selection_and_returns_to_insert() {
    let (mut state, mut handler) = test_state("hello");
    handler.on_key_event(shift_key(KeyCode::Right), &mut state);
    assert_eq!(state.mode, EditorMode::Visual);

    handler.on_key_event(key(KeyCode::Esc), &mut state);

    assert_eq!(state.mode, EditorMode::Insert);
    assert!(state.selection.is_none());
}

/// Reported missing: the command line already had word-wise
/// `Ctrl+Left`/`Right` (`text_field`), the built-in editor never
/// did -- unlike a real shell, `edtui`'s custom keymap has no
/// built-in fallback for an unbound key, so this was a silent
/// no-op rather than falling back to single-character movement.
#[test]
fn ctrl_right_moves_by_a_word_not_one_character() {
    let (mut state, mut handler) = test_state("hello world");
    handler.on_key_event(ctrl_code_key(KeyCode::Right), &mut state);
    assert!(state.cursor.col > 1, "should have moved past just one character: {}", state.cursor.col);
    assert_eq!(state.mode, EditorMode::Insert, "no selection should start");
}

#[test]
fn ctrl_left_moves_back_by_a_word() {
    let (mut state, mut handler) = test_state("hello world");
    state.cursor.col = 11; // end of the line
    handler.on_key_event(ctrl_code_key(KeyCode::Left), &mut state);
    assert!(state.cursor.col < 10, "should have moved back more than one character: {}", state.cursor.col);
}

/// A selection started with `Shift+Right` extends with `Ctrl+Shift+Right`
/// -- both share `state.selection`, no handoff. Here because it covers the
/// seam between the table and `extend_word_selection`.
#[test]
fn switching_from_character_wise_to_word_wise_selection_still_extends() {
    let (mut state, mut handler) = test_state("hello world");
    handler.on_key_event(shift_key(KeyCode::Right), &mut state);
    handler.on_key_event(shift_key(KeyCode::Right), &mut state);
    let after_char_wise = state.selection.as_ref().expect("should have a selection").end;

    extend_word_selection(&mut state, true, false, &mut None);
    let after_word_wise = state.selection.as_ref().expect("should still have a selection").end;
    assert!(after_word_wise.col > after_char_wise.col, "word-wise extend should grow past the character-wise selection: {after_char_wise:?} -> {after_word_wise:?}");
}

#[test]
fn ctrl_z_undoes_last_insert() {
    let (mut state, mut handler) = test_state("");
    handler.on_key_event(key(KeyCode::Char('x')), &mut state);
    assert_eq!(String::from(state.lines.clone()), "x");

    handler.on_key_event(ctrl_key('z'), &mut state);
    assert_eq!(String::from(state.lines.clone()), "");
}
