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

/// Drives one key through the real table, then the same two
/// correction passes `Editor::input` runs, in the same order -- see
/// that method's own doc comment. Most of this module's tests only
/// need the raw table (`handler.on_key_event` alone), but the
/// fresh-Shift-arrow-selection tests specifically exercise the
/// interaction between the table (which no longer performs any
/// `Move` on its own for those keys) and
/// `anchor_fresh_shift_selection`/`wrap_line_boundary_arrow_movement`,
/// which only ever run as part of `Editor::input` -- calling
/// `handler.on_key_event` alone here would only show the table's own
/// half of the fix and silently miss the other half, exactly the
/// mistake a first version of the `Shift+Left` test below made.
fn input(state: &mut EditorState, handler: &mut EditorEventHandler, key_event: crossterm::event::KeyEvent) {
    let cursor_before = state.cursor;
    let mode_before = state.mode;
    handler.on_key_event(key_event, state);

    let freshly_entered_visual = mode_before != EditorMode::Visual && state.mode == EditorMode::Visual;
    let anchored_on_a_real_character = freshly_entered_visual && anchor_fresh_shift_selection(state, key_event.code, cursor_before);

    if !anchored_on_a_real_character {
        wrap_line_boundary_arrow_movement(state, key_event.code, key_event.modifiers, cursor_before);
    }
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
    input(&mut state, &mut handler, shift_key(KeyCode::Right));

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

/// Regression test for the real report on a retest of the fix
/// above: `Shift+Left` was selecting (and copying) the character to
/// the *right* of the cursor, not the left -- the anchor-only
/// behavior that's correct for `Shift+Right` was, at first, reused
/// unmodified for `Shift+Left` too, which is simply the wrong cell
/// for a backward selection. See
/// `shift_select.rs::anchor_fresh_shift_selection`'s own doc comment
/// for the direction-aware fix.
#[test]
fn shift_left_selects_the_character_actually_to_the_left() {
    let (mut state, mut handler) = test_state("Draft architecture");
    state.cursor.col = 6; // right before the 'a' of "architecture"

    input(&mut state, &mut handler, shift_key(KeyCode::Left));

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

/// Regression test for the real report that the `Left`/`Right` fix
/// above got wrongly applied to `Shift+Down` too: it started
/// selecting only one character to the right instead of moving to
/// the next line at the same column, with the second press then
/// landing one column off. `Shift+Down` never had the "N+1, not N"
/// bug in the first place -- the fresh table entry keeps its
/// original chained `Move` (see `shift_select.rs`'s own doc comment
/// for why `Up`/`Down` are scoped out of *that* fix entirely).
///
/// `state.cursor.col` lands one column *short* of the press's own
/// starting column (`shift_select.rs`'s own, separate "exclude the
/// destination row's own landing column" fix, below) -- this is the
/// *data*, kept at the last actually-selected cell same as every
/// other selection in this codebase; the real terminal's own bar
/// cursor still renders one column further right (`Editor::cursor_screen_position`'s
/// existing "+1 while extending forward" shift), so it visually
/// looks like it landed on the same column, matching what a user
/// actually sees.
#[test]
fn shift_down_moves_to_the_next_line_at_the_same_column() {
    let (mut state, mut handler) = test_state("line one\nline two");
    state.cursor.col = 2; // the 'n' of "line", first line

    input(&mut state, &mut handler, shift_key(KeyCode::Down));

    assert_eq!(state.cursor, Index2 { row: 1, col: 1 }, "data should land one column short of the starting column -- the render layer, not the data, is what makes it look aligned");
}

/// A second `Shift+Down` press must keep descending, one line at a
/// time, from the *same* column each time -- not drift further left
/// with each additional row (`MoveDown` never touches `.col` on its
/// own, so the first press's one-time adjustment should just carry
/// forward unchanged).
#[test]
fn repeated_shift_down_keeps_the_same_column() {
    let (mut state, mut handler) = test_state("one\ntwo\nthree");
    state.cursor.col = 1;

    input(&mut state, &mut handler, shift_key(KeyCode::Down));
    input(&mut state, &mut handler, shift_key(KeyCode::Down));

    assert_eq!(state.cursor, Index2 { row: 2, col: 0 }, "should still be one column short of the original column 1, two rows down -- not drifting any further");
}

/// Regression test for the real report against real text: two lines
/// with a word aligned on the identical column
/// (`"config.rs    — LATER..."` / `"editor.rs    — LATER..."`),
/// cursor right before that word on the first line, one
/// `Shift+Down` -- the destination line's own copy of that word must
/// not be swept into the selection too. Checks the actual copied
/// text directly, independent of any `state.selection`/`state.cursor`
/// coordinate assertion, matching this file's own established "check
/// copying separately" pattern.
#[test]
fn shift_down_copies_up_to_but_not_including_the_aligned_word() {
    let (mut state, mut handler) = test_state("aaa LATER one\nbbb LATER two");
    state.cursor.col = 4; // right before 'L', both lines aligned

    input(&mut state, &mut handler, shift_key(KeyCode::Down));

    handler.on_key_event(ctrl_key('c'), &mut state);
    handler.on_key_event(key(KeyCode::End), &mut state);
    handler.on_key_event(ctrl_key('v'), &mut state);

    assert_eq!(
        String::from(state.lines.clone()),
        "aaa LATER one\nbbb LATER twoLATER one\nbbb ",
        "copied text should be \"LATER one\\nbbb \" -- the second line's own \"LATER\" must not be included"
    );
}

/// Mirror-image regression test for `Shift+Up`: cursor right before
/// the aligned word on the *second* line, one `Shift+Up` -- this
/// time it's the *starting* line's own copy of the word (now the
/// selection's bottom edge) that must be excluded, not the
/// destination's (which lands on the first line and should be kept
/// in full).
#[test]
fn shift_up_copies_up_to_but_not_including_the_aligned_words_own_line() {
    let (mut state, mut handler) = test_state("aaa LATER one\nbbb LATER two");
    state.cursor.row = 1;
    state.cursor.col = 4; // right before 'L' on the second line

    input(&mut state, &mut handler, shift_key(KeyCode::Up));

    handler.on_key_event(ctrl_key('c'), &mut state);
    handler.on_key_event(key(KeyCode::End), &mut state);
    handler.on_key_event(ctrl_key('v'), &mut state);

    // Pastes at the end of the *first* line this time (Ctrl+Up left
    // the cursor there, not on the last line) -- the pasted text's
    // own embedded newline pushes the untouched second line down to
    // become a third line, unlike the Down test above where the
    // paste point was already the buffer's last line.
    assert_eq!(
        String::from(state.lines.clone()),
        "aaa LATER oneLATER one\nbbb \nbbb LATER two",
        "copied text should be \"LATER one\\nbbb \" -- the second line's own \"LATER\" (where the press started) must not be included"
    );
}

#[test]
fn select_copy_paste_roundtrip() {
    let (mut state, mut handler) = test_state("hello world");

    // Each Shift+Right now selects exactly one more character than
    // the last (the fresh-selection fix above) -- 5 presses for
    // "hello"'s own 5 characters, not 4 (the old "N+1, not N"
    // quirk this fix removed -- see shift_select.rs).
    for _ in 0..5 {
        handler.on_key_event(shift_key(KeyCode::Right), &mut state); // select "hello"
    }
    handler.on_key_event(ctrl_key('c'), &mut state);
    assert_eq!(state.mode, EditorMode::Insert, "copy should return to typing mode");
    assert!(state.selection.is_none());

    handler.on_key_event(key(KeyCode::End), &mut state);
    handler.on_key_event(ctrl_key('v'), &mut state);

    assert_eq!(String::from(state.lines.clone()), "hello worldhello");
}

/// Independent regression test requested directly after a report
/// that copying "broke" alongside the word-select retraction work
/// (`extend_word_selection`'s "Eighth" doc comment) -- that report
/// turned out to be about the *selection itself* landing wrong, not
/// about `Copy` mishandling a correct selection (confirmed by
/// tracing `CopySelection`, which just reads `state.selection`
/// directly). Still worth pinning down on its own, independent of
/// any coordinate assertion: builds a selection through the real
/// word-select path (`extend_word_selection`, not `Shift+Right`
/// character-wise), retracts one word the same way a real
/// `Ctrl+Shift+Left` after `Ctrl+Shift+Right` would, then copies and
/// pastes back -- so the assertion is on the actual clipboard text
/// content, not on `state.selection`/`state.cursor` numbers.
#[test]
fn word_select_copy_paste_roundtrip() {
    let (mut state, mut handler) = test_state("hello world wide web");

    extend_word_selection(&mut state, true, false); // Ctrl+Shift+Right, selects "hello"
    extend_word_selection(&mut state, true, false); // Ctrl+Shift+Right, extends through " world"
    extend_word_selection(&mut state, false, true); // Ctrl+Shift+Left, retracts back onto "hello"

    handler.on_key_event(ctrl_key('c'), &mut state);
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

/// Regression test for the real, ninth-attempt report against real
/// text: a whole line selected some other way than word-select
/// (character-wise here, matching `word_select::tests::
/// ctrl_shift_left_retracts_fully_even_from_a_never_extended_selection`'s
/// own "Untouched" shape), then one `Ctrl+Shift+Left` -- reported
/// directly against `"theme, 2-column panels, arrow-key"`: the
/// highlight only shrank to `"...arrow-k"` (stopping mid-word,
/// one column short of the `'-'`), because the old fix only handled
/// a *whitespace* gap, not a punctuation one. Selects the whole line
/// one column short of the true end (so the selection's own end sits
/// on `'y'`, the last real character, not the append position past
/// it -- matching how a real "select whole line" action leaves the
/// cursor), then checks the pasted-back text directly, independent
/// of any `state.selection`/`state.cursor` coordinate assertion, per
/// the same "check copying separately" request as
/// `word_select_copy_paste_roundtrip` above.
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

    extend_word_selection(&mut state, false, true); // Ctrl+Shift+Left; retracting=true, as `Editor` computes for an untouched selection

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

/// Regression test for the real report that copying
/// `"Draft architecture derived"` was "unstable" -- traced to the
/// eleventh-attempt bug (`word_select::extend_word_selection`'s own
/// doc comment): retracting past a mid-buffer selection's anchor
/// left a stale anchor behind, so what got copied depended on
/// exactly which combination of forward/backward presses built the
/// selection, not just on where it visibly ended up. `Copy` only
/// ever reads `state.selection` directly (confirmed repeatedly in
/// this file's own history) -- once the anchor itself stops going
/// stale, the copied text should match the same `" "` the direct
/// `word_select` tests pin down, independent of any coordinate
/// assertion.
#[test]
fn word_select_copy_after_retracting_past_the_anchor_matches_the_selection() {
    let (mut state, mut handler) = test_state("Draft architecture derived");
    state.cursor.col = 6; // right before the 'a' of "architecture"

    extend_word_selection(&mut state, true, false); // "architecture"
    extend_word_selection(&mut state, true, false); // "architecture derived"
    extend_word_selection(&mut state, false, true); // retract "derived"
    extend_word_selection(&mut state, false, true); // retract "architecture", crossing the anchor -- " "

    handler.on_key_event(ctrl_key('c'), &mut state);
    handler.on_key_event(key(KeyCode::End), &mut state);
    handler.on_key_event(ctrl_key('v'), &mut state);

    assert_eq!(
        String::from(state.lines.clone()),
        "Draft architecture derived ",
        "should have copied exactly \" \" (one space), not \" a\" (the old stale-anchor bug)"
    );
}

#[test]
fn ctrl_x_cuts_the_selection() {
    let (mut state, mut handler) = test_state("hello world");

    for _ in 0..6 {
        handler.on_key_event(shift_key(KeyCode::Right), &mut state); // select "hello " -- 6 presses for 6 characters now, see select_copy_paste_roundtrip
    }
    handler.on_key_event(ctrl_key('x'), &mut state);

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
/// `Ctrl+Left`/`Right` (`text_field.rs`), the built-in editor never
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

/// A selection started character-wise (`Shift+Right`) should still
/// extend correctly once switched to word-wise (`Ctrl+Shift+Right`)
/// mid-selection -- both share the same `state.selection`, so
/// there's no special handoff needed, but worth pinning down
/// directly since a user is likely to mix the two in practice (a
/// few characters, then "grab the rest of this word"). Lives here
/// rather than in `word_select`'s own test module since it's really
/// exercising the handoff *between* this table and that function,
/// not either one in isolation.
#[test]
fn switching_from_character_wise_to_word_wise_selection_still_extends() {
    let (mut state, mut handler) = test_state("hello world");
    handler.on_key_event(shift_key(KeyCode::Right), &mut state);
    handler.on_key_event(shift_key(KeyCode::Right), &mut state);
    let after_char_wise = state.selection.as_ref().expect("should have a selection").end;

    extend_word_selection(&mut state, true, false);
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
