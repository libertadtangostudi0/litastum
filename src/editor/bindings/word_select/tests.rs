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

/// Regression test for the real, repeatedly-reported bug (and its
/// own, differently-shaped regression once fixed the first time):
/// one `Ctrl+Shift+Right` press on "hello world" must select exactly
/// "hello" -- neither reaching into the 'w' of "world" (the original
/// report) nor stopping one column short, on the trailing space
/// (a later attempt's own overcorrection, also reported directly:
/// selection must track only where the cursor itself travels,
/// nothing added on either side). See `extend_word_selection`'s own
/// doc comment for the earlier attempts that got this wrong, in both
/// directions.
#[test]
fn ctrl_shift_right_does_not_select_into_the_next_word() {
    let mut state = state_for("hello world", 0);
    extend_word_selection(&mut state, true, false, &mut None);
    let selection = state.selection.expect("should have started a selection");
    assert_eq!(selection.end.col, 4, "should land on the last letter of \"hello\" -- not the space after it, not the 'w' of \"world\"");
    assert_eq!(selection.end, state.cursor, "the selection's own end must always equal the cursor");
}

/// Regression test for a real report: an earlier fix (a corrective
/// `MoveBackward(1)` chained after `MoveWordForward`) left the
/// cursor sitting on the space between words, and the *next*
/// `Ctrl+Shift+Right` press's own word-scan started from that same
/// space -- crossing back to the same 'w' and then immediately
/// stepping back again netted zero movement, so the second press
/// was silently ignored ("иногда игнорится" -- reported directly).
/// A real editor's word-selection must extend further on every
/// press, never stall.
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

/// Regression test for the real, seventh-attempt report: starting a
/// *fresh* backward selection while the cursor already sits at the
/// very start of a word (exactly where a prior plain `Ctrl+Right`
/// word move leaves it) must not drag that word's own first
/// character into the selection. `state_for(.., 6)` puts the cursor
/// on the `'w'` of "world" -- itself already the start of that word,
/// same shape as landing on the `'t'` of "the" in `"loaded the"`
/// after a plain `Ctrl+Right`. Reported directly: selecting backward
/// from there grabbed `"loaded t"`, not `"loaded "`.
#[test]
fn ctrl_shift_left_from_a_word_start_does_not_grab_that_words_first_letter() {
    let mut state = state_for("hello world", 6); // cursor on 'w', the very start of "world"
    extend_word_selection(&mut state, false, false, &mut None);
    let selection = state.selection.expect("should have started a selection");
    assert_eq!(selection.start.col, 5, "anchor should trim back into the space, not ride along on 'w'");
    assert_eq!(selection.end.col, 0, "should land on 'h', the start of \"hello\"");
    assert_eq!(state.cursor.col, 0);
}

/// Regression test for the real, twelfth-attempt report: starting a
/// *fresh* backward selection while the cursor rests on the whitespace
/// right after a word (the ordinary place a cursor sits once it's moved
/// or typed past one) must not drag that whitespace into the selection
/// either. Reported directly against
/// `"Draft architecture derived from the planning chat + the Far
/// Manager UI"`, cursor placed right after "derived"'s own last letter
/// (i.e. resting on the space before "from"): selecting backward from
/// there grabbed `"derived "` (trailing space included), not
/// `"derived"`.
#[test]
fn ctrl_shift_left_from_the_space_after_a_word_does_not_grab_that_space() {
    let mut state = state_for("Draft architecture derived from the planning chat", 26); // the space right after "derived"
    extend_word_selection(&mut state, false, false, &mut None);
    let selection = state.selection.expect("should have started a selection");
    assert_eq!(selection.start.col, 25, "anchor should trim back onto the 'd' of \"derived\", not ride along on the space");
    assert_eq!(selection.end.col, 19, "should land on the 'd' that starts \"derived\"");
    assert_eq!(state.cursor.col, 19);
}

/// Regression test for the real, thirteenth-attempt report: a fresh
/// backward selection starting *mid-word*, with no whitespace or word
/// boundary anywhere nearby, must trim the anchor too -- same as every
/// other shape of "the anchor holds a real character" (Seventh,
/// Twelfth). Reported directly against plain "roadmap", cursor placed
/// between 'd' and 'm' (i.e. resting on 'm' itself): selecting backward
/// grabbed `"roadm"` (the anchor's own 'm' included), not `"road"`. An
/// earlier version of this test (before the general fix landed) had
/// asserted the *opposite* -- that a mid-word anchor legitimately
/// belongs to the selection and must not be trimmed; that assumption
/// was the bug, never verified against a real report until this one.
#[test]
fn ctrl_shift_left_from_mid_word_trims_the_anchor_too() {
    let mut state = state_for("roadmap", 4); // cursor on 'm', between 'd' and 'm'
    extend_word_selection(&mut state, false, false, &mut None);
    let selection = state.selection.expect("should have started a selection");
    assert_eq!(selection.start.col, 3, "anchor should trim back onto 'd', not ride along on 'm'");
    assert_eq!(selection.end.col, 0, "should land on 'r', the start of \"roadmap\"");
    assert_eq!(state.cursor.col, 0);
}

/// Regression test for the real, eighth-attempt report -- including
/// its own first, overcorrected fix (see `extend_word_selection`'s
/// own doc comment). *Retracting* a selection that was built by
/// extending forward word-by-word must remove a whole word's own
/// text per press, landing on the separating space in front of it --
/// not stopping one character short, still inside the word just
/// "removed" (`"hello world f"`, the original bug), and not
/// swallowing the space too (`"hello world"` with no trailing space,
/// the first fix's own overcorrection). "hello world foo", built up
/// one word at a time with `Right`, then retracted once with `Left`,
/// should land right on the space right after "world" --
/// `"hello world "`.
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

/// Regression test for the real, fifteenth-attempt report: the
/// simplest possible round trip -- extend one word forward, then
/// immediately retract that exact same word -- must return to nothing
/// selected, not leave a stray character behind. `"Draft architecture"`,
/// cursor right before "architecture" (column 6): `Ctrl+Shift+Right`
/// selects "architecture"; `Ctrl+Shift+Left` once should undo it
/// completely, landing back exactly where the `Right` press started,
/// with no selection at all -- not `" "` (the space before
/// "architecture"), which an earlier version of the retraction fix
/// (see `extend_word_selection`'s own doc comment, "Eleventh") produced
/// instead.
#[test]
fn ctrl_shift_right_then_left_returns_to_nothing_selected() {
    let mut state = state_for("Draft architecture", 6); // right before "architecture"

    extend_word_selection(&mut state, true, false, &mut None); // Ctrl+Shift+Right, selects "architecture"
    extend_word_selection(&mut state, false, true, &mut None); // Ctrl+Shift+Left, should undo it completely

    assert_eq!(state.mode, EditorMode::Insert, "should have closed the selection entirely, not left \" \" selected");
    assert!(state.selection.is_none());
    assert_eq!(state.cursor.col, 6, "should be back exactly where the Right press started");
}

/// Regression test for the real, sixteenth-attempt report: retracing a
/// *backward*-built selection must land back on the exact original
/// starting column, not the trimmed anchor. `"derived"`, cursor between
/// 'i' and 'v' (column 4): `Ctrl+Shift+Left` selects "deri" (trims the
/// anchor to column 3, per `trim_anchor_off_a_word_it_never_visited`),
/// `Ctrl+Shift+Right` should return the cursor to column 4 exactly --
/// not column 3 (one short), which an earlier version of
/// `retreat_forward_through_a_backward_walk` produced by snapping to
/// the trimmed `selection.start` directly instead of the real,
/// untrimmed starting column (`true_anchor`, threaded through here the
/// same way `Editor::word_select_true_anchor` is threaded through in
/// real use).
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

/// One further `Ctrl+Shift+Right` past the return above starts a fresh
/// *forward* selection from the true original column -- matching VS
/// Code's own "reflect" behavior for this same case (confirmed directly
/// against it): once back where the backward selection started, a
/// further `Right` selects "ved", the mirror image of "deri" around the
/// original cursor position. Takes one extra press to get there
/// compared to VS Code's own single `Right` -- this codebase's own
/// retracing step and a fresh extension are two separate presses here,
/// not one combined action.
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

/// Regression test for the real, fourth-shape report: a selection
/// that was *never* built by word-wise `Right` presses at all (e.g.
/// a whole line selected some other way) still needs to retract
/// fully on its very first `Ctrl+Shift+Left` -- `retracting=true` is
/// exactly what `Editor::extend_word_selection` computes for "this
/// selection's `WordSelectTouch` is `Untouched`" (see that method's
/// own doc comment), regardless of `forward` ever having fired.
/// "hello world, foo" -- a comma directly against "world" with no
/// space, matching the real report's own trailing punctuation --
/// with the cursor starting right on the comma: retracting once
/// should land on the space right after "world", same as the
/// plain-word case above, not stall on `'w'`/`'d'` the way a
/// character-classification guess (this fix's own second attempt)
/// got wrong for exactly this shape.
#[test]
fn ctrl_shift_left_retracts_fully_even_from_a_never_extended_selection() {
    let mut state = state_for("hello world, foo", 11); // cursor right on the ','
    extend_word_selection(&mut state, false, true, &mut None); // retracting=true -- as if Editor found this selection's touch to be Untouched
    assert_eq!(state.cursor.col, 5, "should land on the space right after \"world\" -- \"hello world \", comma and all removed");
}

/// Regression test for the real, ninth-attempt report: retracting
/// across a *punctuation* separator (not whitespace) must land on
/// the separator itself too, same as the whitespace case above --
/// reported directly against `"...arrow-key"`: one retract landed on
/// `'k'` (`"...arrow-k"`, one column short) instead of on the `'-'`
/// itself (`"...arrow-"`), because the old fix only stepped onto a
/// *whitespace* gap. `"arrow-key"` with the cursor on `'y'` (the last
/// char of "key", as an already-fully-selected line would have it,
/// `retracting=true` simulating `WordSelectTouch::Untouched`) --
/// retracting once should remove "key" and land right on the `'-'`.
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

/// Regression test for the real retest that caught the tenth fix's
/// own first attempt still failing -- the actual reported flow
/// wasn't two fresh `Ctrl+Shift+Left` presses from column 0, it was
/// selecting the line's first word via `Ctrl+Shift+Right`, then
/// retracting it with `Ctrl+Shift+Left`: `MoveWordBackward` genuinely
/// moves here (from "Draft"'s last letter back to its first), so the
/// zero-progress check never fired, but the cursor still lands
/// exactly back on the selection's own anchor (column 0) -- the same
/// phantom "D" left selected, copyable, reported again verbatim.
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

/// Regression test for the real, fifteenth-attempt report -- and its
/// own predecessor, the eleventh-attempt fix this one revised (see
/// `extend_word_selection`'s own doc comment for the full story).
/// Retracting *past* a mid-buffer selection's own anchor must close the
/// selection entirely -- nothing is left to give back once
/// `MoveWordBackward` lands exactly on the anchor. `"Draft architecture
/// derived"`, cursor placed right before the `'a'` of "architecture"
/// (column 6): two `Ctrl+Shift+Right` presses select `"architecture
/// derived"`; two `Ctrl+Shift+Left` presses should retract both words
/// fully back to nothing -- not leave `" "` (the space beyond the
/// anchor) selected, which is what an earlier version of this fix
/// deliberately produced, until a simpler report (one word extended
/// then immediately retracted) showed that broke the most basic
/// round-trip case there is.
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

/// One further `Ctrl+Shift+Left` past the closing above starts an
/// entirely ordinary *fresh* backward selection from the same anchor
/// position (the previous press just returned `state.mode` to
/// `Insert`) -- walking into "Draft" and landing at column 0, with
/// `trim_anchor_off_a_word_it_never_visited` trimming the fresh anchor
/// back into the gap, giving `"Draft "` in full. Byte-for-byte the same
/// result the old "claims new territory" fix produced for this same
/// third press, confirming nothing was actually lost by simplifying the
/// second press to a clean close instead.
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

/// The fix must not fire when there was never anything to retract in
/// the first place -- a selection built entirely by `Left` presses
/// (never touching `Right`) always lands each press on a word's own
/// *start* (`ctrl_shift_left_selects_the_whole_previous_word_with_no_extra_character`,
/// `repeated_ctrl_shift_left_keeps_extending_the_selection_backward`),
/// never its last character, so the new guard should never trigger
/// for it -- pinned down directly here rather than only inferred
/// from those other tests still passing.
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

// No general `Right`-then-`Left` round-trip guarantee for a *fresh*
// selection: forward still lands on a word's own *last* character
// (`ctrl_shift_right_does_not_select_into_the_next_word`) while a
// fresh backward selection lands on a word's *first* character
// (`ctrl_shift_left_selects_the_whole_previous_word_with_no_extra_character`)
// -- two individually-correct conventions, deliberately not unified
// onto one shared grid (see `extend_word_selection`'s own doc
// comment). What *is* now guaranteed, per the "Eighth" tests above:
// retracting an *already-open* selection removes one whole word's
// own text per `Left` press, landing on the space in front of it --
// not exactly the cell a matching `Right` press had left the cursor
// on (that was the first, overcorrected attempt at this fix), one
// column further back, onto the separator itself.

/// Regression test for the real, reported bug ("Seventeenth" in
/// `extend_word_selection`'s own doc comment): forward extension used
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

/// Same root cause, a different non-ASCII character -- a Cyrillic run
/// this time, to confirm the fix isn't specific to punctuation-shaped
/// non-ASCII characters like an em dash.
#[test]
fn ctrl_shift_right_selects_over_a_cyrillic_word_instead_of_getting_stuck() {
    let mut state = state_for("hello мир world", 0);

    extend_word_selection(&mut state, true, false, &mut None); // "hello"
    let after_hello = state.cursor.col;

    extend_word_selection(&mut state, true, false, &mut None); // "мир"

    assert!(state.cursor.col > after_hello, "should have advanced onto/through \"мир\", not stayed stuck on \"hello\"");
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
