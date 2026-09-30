use edtui::{EditorMode, EditorState, Lines};

use super::extend_word_selection;

/// A raw `EditorState` with no key handler attached -- every test
/// here calls `extend_word_selection` directly rather than going
/// through `standard_key_handler`, so there's no need for the
/// handler/clipboard setup `bindings::tests::test_state` has.
fn state_for(contents: &str, cursor_col: usize) -> EditorState {
    let mut state = EditorState::new(Lines::from(contents));
    state.mode = EditorMode::Insert;
    state.cursor.col = cursor_col;
    state
}

#[test]
fn ctrl_shift_right_selects_by_a_word() {
    let mut state = state_for("hello world", 0);
    extend_word_selection(&mut state, true, false, &mut None);
    assert_eq!(state.mode, EditorMode::Visual, "should start a selection, same as plain Shift+Right");
    assert_eq!(state.cursor.col, 4, "cursor should land on the last letter of \"hello\" -- no trailing space, no next word");
    assert!(state.selection.is_some());
}

/// One `Ctrl+Shift+Right` on "hello world" selects exactly "hello": not
/// the `w` (the original report), not stopping on the space (a later
/// overcorrection). History: docs/history/word-select.md.
#[test]
fn ctrl_shift_right_does_not_select_into_the_next_word() {
    let mut state = state_for("hello world", 0);
    extend_word_selection(&mut state, true, false, &mut None);
    let selection = state.selection.expect("should have started a selection");
    assert_eq!(selection.end.col, 4, "should land on the last letter of \"hello\" -- not the space after it, not the 'w' of \"world\"");
    assert_eq!(selection.end, state.cursor, "the selection's own end must always equal the cursor");
}

/// "Second": each press must extend further. A corrective
/// `MoveBackward(1)` left the cursor on the space, and the next press
/// cancelled itself out.
#[test]
fn repeated_ctrl_shift_right_keeps_extending_the_selection() {
    let mut state = state_for("hello world wide web", 0);

    extend_word_selection(&mut state, true, false, &mut None);
    let after_first = state.selection.as_ref().expect("should have started a selection").end;

    extend_word_selection(&mut state, true, false, &mut None);
    let after_second = state.selection.as_ref().expect("should still have a selection").end;
    assert!(after_second.col > after_first.col, "second press should extend further, not stall: {after_first:?} -> {after_second:?}");

    extend_word_selection(&mut state, true, false, &mut None);
    let after_third = state.selection.as_ref().expect("should still have a selection").end;
    assert!(after_third.col > after_second.col, "third press should extend further still: {after_second:?} -> {after_third:?}");
}

/// `Ctrl+Shift+Left` needs no equivalent trim -- landing on the
/// *start* of the target word going backward is already exactly
/// the character that should be included (confirmed directly: no
/// extra character grabbed).
#[test]
fn ctrl_shift_left_selects_the_whole_previous_word_with_no_extra_character() {
    let mut state = state_for("hello world", 11); // end of the line
    extend_word_selection(&mut state, false, false, &mut None);
    let selection = state.selection.expect("should have started a selection");
    assert_eq!(selection.end.col, 6, "should land right at the start of \"world\"");
}

/// `MoveWordBackward` already always self-skips whitespace before
/// scanning and lands on a real word character, never on
/// whitespace -- confirmed directly that repeated `Ctrl+Shift+Left`
/// presses walk back cleanly too, same shape as
/// `repeated_ctrl_shift_right_...` above.
#[test]
fn repeated_ctrl_shift_left_keeps_extending_the_selection_backward() {
    let mut state = state_for("hello world wide web", 20); // end of the line

    extend_word_selection(&mut state, false, false, &mut None);
    let after_first = state.selection.as_ref().expect("should have started a selection").end;

    extend_word_selection(&mut state, false, false, &mut None);
    let after_second = state.selection.as_ref().expect("should still have a selection").end;
    assert!(after_second.col < after_first.col, "second press should extend further back, not stall: {after_first:?} -> {after_second:?}");

    extend_word_selection(&mut state, false, false, &mut None);
    let after_third = state.selection.as_ref().expect("should still have a selection").end;
    assert!(after_third.col < after_second.col, "third press should extend further back still: {after_second:?} -> {after_third:?}");
}

/// "Seventh": a fresh backward selection starting on a word's first
/// character (where `Ctrl+Right` leaves it) doesn't include it --
/// `"loaded "`, not `"loaded t"`.
#[test]
fn ctrl_shift_left_from_a_word_start_does_not_grab_that_words_first_letter() {
    let mut state = state_for("hello world", 6); // cursor on 'w', the very start of "world"
    extend_word_selection(&mut state, false, false, &mut None);
    let selection = state.selection.expect("should have started a selection");
    assert_eq!(selection.start.col, 5, "anchor should trim back into the space, not ride along on 'w'");
    assert_eq!(selection.end.col, 0, "should land on 'h', the start of \"hello\"");
    assert_eq!(state.cursor.col, 0);
}

/// "Twelfth": a fresh backward selection starting on the space after a
/// word doesn't include it -- `"derived"`, not `"derived "`.
#[test]
fn ctrl_shift_left_from_the_space_after_a_word_does_not_grab_that_space() {
    let mut state = state_for("Draft architecture derived from the planning chat", 26); // the space right after "derived"
    extend_word_selection(&mut state, false, false, &mut None);
    let selection = state.selection.expect("should have started a selection");
    assert_eq!(selection.start.col, 25, "anchor should trim back onto the 'd' of \"derived\", not ride along on the space");
    assert_eq!(selection.end.col, 19, "should land on the 'd' that starts \"derived\"");
    assert_eq!(state.cursor.col, 19);
}

/// "Thirteenth": a fresh backward selection starting mid-word trims the
/// anchor too -- `"road"`, not `"roadm"`. An earlier version of this test
/// asserted the opposite; that assumption was the bug.
#[test]
fn ctrl_shift_left_from_mid_word_trims_the_anchor_too() {
    let mut state = state_for("roadmap", 4); // cursor on 'm', between 'd' and 'm'
    extend_word_selection(&mut state, false, false, &mut None);
    let selection = state.selection.expect("should have started a selection");
    assert_eq!(selection.start.col, 3, "anchor should trim back onto 'd', not ride along on 'm'");
    assert_eq!(selection.end.col, 0, "should land on 'r', the start of \"roadmap\"");
    assert_eq!(state.cursor.col, 0);
}

/// "Eighth": retracting a forward-built selection removes a whole word per
/// press and lands on the separator -- `"hello world "`, neither
/// `"hello world f"` (the bug) nor `"hello world"` (the first fix).
#[test]
fn ctrl_shift_left_retracts_a_whole_word_onto_the_separating_space() {
    let mut state = state_for("hello world foo", 0);

    extend_word_selection(&mut state, true, false, &mut None); // "hello"
    extend_word_selection(&mut state, true, false, &mut None); // "hello world"
    extend_word_selection(&mut state, true, false, &mut None); // "hello world foo"

    extend_word_selection(&mut state, false, true, &mut None); // retract "foo" -- `retracting=true`, simulating what `Editor::extend_word_selection` computes after those three `Right` presses

    assert_eq!(state.cursor.col, 11, "should land on the space right after \"world\" -- \"hello world \", not \"hello world\" or \"hello world f\"");
    let selection = state.selection.expect("should still have a selection");
    assert_eq!(selection.end, state.cursor, "the selection's own end must always equal the cursor, same invariant as everywhere else");
}

/// "Fifteenth": one word forward and straight back leaves nothing
/// selected, not the space before it ("Eleventh"'s result).
#[test]
fn ctrl_shift_right_then_left_returns_to_nothing_selected() {
    let mut state = state_for("Draft architecture", 6); // right before "architecture"

    extend_word_selection(&mut state, true, false, &mut None); // Ctrl+Shift+Right, selects "architecture"
    extend_word_selection(&mut state, false, true, &mut None); // Ctrl+Shift+Left, should undo it completely

    assert_eq!(state.mode, EditorMode::Insert, "should have closed the selection entirely, not left \" \" selected");
    assert!(state.selection.is_none());
    assert_eq!(state.cursor.col, 6, "should be back exactly where the Right press started");
}

/// "Sixteenth": retracing a backward-built selection returns to the true
/// starting column (4 in "derived"), not the trimmed anchor (3) --
/// `true_anchor` is threaded like `Editor::word_select_true_anchor`.
#[test]
fn ctrl_shift_right_after_left_returns_to_the_true_starting_column() {
    let mut state = state_for("derived", 4); // between 'i' and 'v'
    let mut true_anchor = None;

    extend_word_selection(&mut state, false, false, &mut true_anchor); // Ctrl+Shift+Left, selects "deri"
    let selection = state.selection.as_ref().expect("should have started a selection");
    assert_eq!(selection.start.col, 3, "sanity check -- \"deri\" trims the anchor to column 3");

    extend_word_selection(&mut state, true, true, &mut true_anchor); // Ctrl+Shift+Right, should retrace back to column 4

    assert_eq!(state.mode, EditorMode::Insert, "should have closed the selection entirely");
    assert!(state.selection.is_none());
    assert_eq!(state.cursor.col, 4, "should be back at the exact original column, not the trimmed anchor (3)");
}

/// A further `Ctrl+Shift+Right` from there starts a fresh forward
/// selection ("ved"), mirroring VS Code -- one press later than VS Code,
/// since retracing and extending are separate presses here.
#[test]
fn ctrl_shift_right_one_more_press_after_the_return_reflects_forward() {
    let mut state = state_for("derived", 4);
    let mut true_anchor = None;

    extend_word_selection(&mut state, false, false, &mut true_anchor); // "deri"
    extend_word_selection(&mut state, true, true, &mut true_anchor); // back to column 4, closed
    extend_word_selection(&mut state, true, false, &mut true_anchor); // fresh Right -- "ved"

    let selection = state.selection.expect("should have started a fresh forward selection");
    assert_eq!(selection.start.col, 4, "anchor should be the true original column");
    assert_eq!(selection.end.col, 6, "should land on the last letter of \"derived\"");
    assert_eq!(state.cursor.col, 6);
}

/// The retraction above must keep working word-by-word (plus each
/// word's own leading space) on further presses, not just the one
/// right after a `Right` -- the second press's `cursor_before` is
/// itself the *space* the first press landed on, a different shape
/// than "a word's own last character," and both need to trigger the
/// same correction.
#[test]
fn repeated_ctrl_shift_left_after_ctrl_shift_right_keeps_retracting_word_plus_space() {
    let mut state = state_for("hello world foo", 0);

    extend_word_selection(&mut state, true, false, &mut None); // "hello"
    extend_word_selection(&mut state, true, false, &mut None); // "hello world"
    extend_word_selection(&mut state, true, false, &mut None); // "hello world foo"

    // `retracting=true` on both -- `Editor::extend_word_selection` never
    // clears the flag on a backward press, only a fresh selection or a
    // `Right` press does, so an entire streak of `Left` presses after
    // one-or-more `Right`s all see it `true`, not just the first.
    extend_word_selection(&mut state, false, true, &mut None); // retract "foo" -> "hello world "
    extend_word_selection(&mut state, false, true, &mut None); // retract "world " -> "hello "

    assert_eq!(state.cursor.col, 5, "two Left presses after three Right presses should land on the space right after \"hello\" -- \"hello \"");
}

/// A selection never built by word-wise `Right` (`Untouched`, so
/// `retracing = true`) retracts fully on the first `Ctrl+Shift+Left`,
/// also across trailing punctuation ("world,") -- where a
/// character-classification guess stalled.
#[test]
fn ctrl_shift_left_retracts_fully_even_from_a_never_extended_selection() {
    let mut state = state_for("hello world, foo", 11); // cursor right on the ','
    extend_word_selection(&mut state, false, true, &mut None); // retracting=true -- as if Editor found this selection's touch to be Untouched
    assert_eq!(state.cursor.col, 5, "should land on the space right after \"world\" -- \"hello world \", comma and all removed");
}

/// "Ninth": retracting across punctuation lands on the separator --
/// `"arrow-"`, not `"arrow-k"`.
#[test]
fn ctrl_shift_left_retracts_fully_across_a_punctuation_separator_too() {
    let mut state = state_for("arrow-key", 8); // cursor on 'y', the last char of "key"
    extend_word_selection(&mut state, false, true, &mut None);
    assert_eq!(
        state.cursor.col, 5,
        "should land on '-' itself -- \"arrow-\", not \"arrow-k\" (stopping mid-word, the old bug) \
         or \"arrow\" (swallowing the separator too)"
    );
}

/// Regression test for the real, tenth-attempt report: starting a
/// *fresh* backward selection at the very start of the buffer (no
/// previous word to jump to at all) must not open a phantom
/// one-character selection and get stuck there. Reported directly
/// against `"Draft architecture"`: two `Ctrl+Shift+Left` presses
/// from column 0 left `"D"` selected, wanted none.
#[test]
fn ctrl_shift_left_at_the_very_start_of_the_buffer_selects_nothing() {
    let mut state = state_for("Draft architecture", 0);

    extend_word_selection(&mut state, false, false, &mut None);
    assert_eq!(state.mode, EditorMode::Insert, "should not have opened a selection with nowhere to go");
    assert!(state.selection.is_none(), "should not have selected the first character just by anchoring on it");

    extend_word_selection(&mut state, false, false, &mut None);
    assert_eq!(state.mode, EditorMode::Insert, "a second press should hit the same wall, not accumulate a selection");
    assert!(state.selection.is_none());
}

/// Same shape, different real text -- pinned down independently
/// since the first report's own retest happened to reuse this exact
/// second string.
#[test]
fn ctrl_shift_left_at_the_very_start_of_the_buffer_selects_nothing_on_other_text_too() {
    let mut state = state_for("current scaffold", 0);

    extend_word_selection(&mut state, false, false, &mut None);
    extend_word_selection(&mut state, false, false, &mut None);

    assert_eq!(state.mode, EditorMode::Insert);
    assert!(state.selection.is_none(), "\"c\" must not end up selected -- there was nowhere for either press to move to");
}

/// "Tenth", retest: select the first word with `Ctrl+Shift+Right`, then
/// retract it. `MoveWordBackward` really moves, so the zero-progress check
/// never fired -- but the cursor lands on the anchor and must close,
/// not leave a phantom "D".
#[test]
fn ctrl_shift_left_retracting_the_first_word_of_the_buffer_selects_nothing() {
    let mut state = state_for("Draft architecture", 0);

    extend_word_selection(&mut state, true, false, &mut None); // Ctrl+Shift+Right, selects "Draft"
    assert!(state.selection.is_some(), "sanity check -- should have a real selection to retract");

    extend_word_selection(&mut state, false, true, &mut None); // Ctrl+Shift+Left, retracts "Draft" -- retracting=true, as `Editor` computes after a forward press

    assert_eq!(state.mode, EditorMode::Insert, "retracting the only word back to its own start should close the selection, not leave \"D\" behind");
    assert!(state.selection.is_none());
}

/// Same shape as the test above, different real text -- matches the
/// report's own second example verbatim.
#[test]
fn ctrl_shift_left_retracting_the_first_word_of_the_buffer_selects_nothing_on_other_text_too() {
    let mut state = state_for("current scaffold", 0);

    extend_word_selection(&mut state, true, false, &mut None); // "current"
    extend_word_selection(&mut state, false, true, &mut None); // retracts it fully

    assert_eq!(state.mode, EditorMode::Insert);
    assert!(state.selection.is_none(), "\"c\" must not end up selected after retracting the whole first word");
}

/// "Fifteenth" (revising "Eleventh"): retracting two words back past the
/// anchor closes the selection entirely, not leaving `" "` selected.
#[test]
fn ctrl_shift_left_retracting_past_a_mid_buffer_anchor_closes_the_selection() {
    let mut state = state_for("Draft architecture derived", 6);

    extend_word_selection(&mut state, true, false, &mut None); // "architecture"
    extend_word_selection(&mut state, true, false, &mut None); // "architecture derived"

    extend_word_selection(&mut state, false, true, &mut None); // retract "derived"
    extend_word_selection(&mut state, false, true, &mut None); // retract "architecture", crossing the anchor

    assert_eq!(state.mode, EditorMode::Insert, "should have closed the selection entirely, not left \" \" behind");
    assert!(state.selection.is_none());
}

/// A further `Ctrl+Shift+Left` then starts an ordinary fresh backward
/// selection: `"Draft "`, the same result the old "claim new territory"
/// fix gave -- nothing lost by simplifying.
#[test]
fn ctrl_shift_left_one_more_press_past_the_closing_selects_the_previous_word() {
    let mut state = state_for("Draft architecture derived", 6);

    extend_word_selection(&mut state, true, false, &mut None); // "architecture"
    extend_word_selection(&mut state, true, false, &mut None); // "architecture derived"
    extend_word_selection(&mut state, false, true, &mut None); // retract "derived"
    extend_word_selection(&mut state, false, true, &mut None); // retract "architecture" -- closes entirely

    extend_word_selection(&mut state, false, true, &mut None); // one more Left -- a fresh press now

    let selection = state.selection.expect("should have started a fresh selection");
    assert_eq!(state.cursor.col, 0, "should land at the very start of \"Draft\"");
    assert_eq!(selection.start.col, 5, "fresh anchor should trim back into the gap, giving \"Draft \" in full");
}

/// The retraction guard never fires for a selection built only by `Left`
/// presses, which always land on a word's start.
#[test]
fn pure_backward_selection_is_unaffected_by_the_retraction_fix() {
    let mut state = state_for("hello world wide web", 20); // end of the line

    extend_word_selection(&mut state, false, false, &mut None); // "web"
    let after_first = state.selection.as_ref().expect("should have a selection").end.col;
    extend_word_selection(&mut state, false, false, &mut None); // "wide web"
    let after_second = state.selection.as_ref().expect("should have a selection").end.col;

    assert_eq!(after_first, 17, "should land on 'w', the start of \"web\" -- unaffected by the retraction fix");
    assert_eq!(after_second, 12, "should land on 'w', the start of \"wide\" -- still just plain MoveWordBackward");
}

// No `Right`-then-`Left` round-trip guarantee for a fresh selection:
// forward lands on a word's last character, a fresh backward one on its
// first -- two conventions, deliberately not unified. Retracting an open
// selection does remove one whole word per `Left`, landing on the
// separator. History: docs/history/word-select.md.

/// Regression test for the real, reported bug ("Seventeenth" in
/// `docs/history/word-select.md`): forward extension used
/// to get permanently stuck the instant the very next real character
/// was non-ASCII (an em dash here) -- every further `Ctrl+Shift+Right`
/// press did nothing at all once the selection reached it. Reported
/// against real prose almost identical to this.
#[test]
fn ctrl_shift_right_selects_over_an_em_dash_instead_of_getting_stuck() {
    let mut state = state_for("in Russian by default — commit messages", 0);

    for _ in 0..4 {
        extend_word_selection(&mut state, true, false, &mut None); // "in", "Russian", "by", "default"
    }
    let after_default = state.cursor.col;

    extend_word_selection(&mut state, true, false, &mut None); // the em dash itself

    assert!(state.cursor.col > after_default, "should have moved past \"default\" onto the em dash, not stayed stuck");
    assert_eq!(state.selection.as_ref().unwrap().end, state.cursor);

    // And a further press should keep going into "commit" -- the stall
    // used to persist forever once hit, not just for the one press
    // that landed on the non-ASCII character.
    extend_word_selection(&mut state, true, false, &mut None);
    assert!(state.cursor.col > after_default + 1, "should keep extending into \"commit\" after the em dash, not stay stuck there either");
}

/// Same root cause, a different non-ASCII character -- a run of letters
/// outside the Latin alphabet this time, to confirm the fix isn't
/// specific to punctuation-shaped non-ASCII characters like an em dash.
#[test]
fn ctrl_shift_right_selects_over_a_non_latin_word_instead_of_getting_stuck() {
    let mut state = state_for("hello κόσμος world", 0);

    extend_word_selection(&mut state, true, false, &mut None); // "hello"
    let after_hello = state.cursor.col;

    extend_word_selection(&mut state, true, false, &mut None); // "κόσμος"

    assert!(state.cursor.col > after_hello, "should have advanced onto/through \"κόσμος\", not stayed stuck on \"hello\"");
}

/// A selection that reaches a non-ASCII run and then retracts with
/// `Ctrl+Shift+Left` should give it back cleanly, same as any other
/// character run -- confirms the forward-only fix above doesn't leave
/// the selection in a state the existing retraction logic can't handle.
#[test]
fn ctrl_shift_left_retracts_back_off_an_em_dash_normally() {
    let mut state = state_for("default — commit", 0);
    for _ in 0..2 {
        extend_word_selection(&mut state, true, false, &mut None); // "default", then the em dash
    }
    let reached = state.cursor.col;

    extend_word_selection(&mut state, false, true, &mut None);

    assert!(state.cursor.col < reached, "should have retracted back off the em dash");
}
