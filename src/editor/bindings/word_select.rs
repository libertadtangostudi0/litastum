use edtui::actions::{Execute, MoveWordBackward, MoveWordForwardToEndOfWord, SwitchMode};
use edtui::{EditorMode, EditorState, Index2};

/// `Ctrl+Shift+Left`/`Right` -- word-wise selection. Called directly
/// from `editor_keymap.rs::handle_editor_key`, ahead of `Editor::input`
/// and `bindings::standard_key_handler`'s own declarative table (see
/// that function's own comment for why): no fixed sequence of `edtui`'s
/// own `Action`s gives VS Code's actual behavior (below is the history
/// of why, kept because it's non-obvious and easy to re-attempt by
/// accident) --
///
/// 1. Plain `MoveWordForward`/`MoveWordBackward`: lands exactly on the
///    first character of the adjacent word. `edtui`'s selection is
///    `[start, cursor]`, *inclusive* on the cursor end (same trap as
///    plain `Shift+Right`, see `.claude/rules/litastum-stack.md`) --
///    extending `Right` visibly grabbed that character into the
///    highlight AND into what `Copy` produced -- reported directly,
///    repeatedly, across several real files ("planning chat " copied
///    as "planning chat +").
/// 2. Chaining a corrective `MoveBackward(1)` after `MoveWordForward`
///    fixed that, but left the cursor sitting mid-whitespace -- the
///    *next* press's own class-detection started from that character,
///    immediately re-crossed the same gap and cancelled itself back to
///    the same spot (repeated presses silently ignored, reported
///    directly).
/// 3. Swapping to `MoveWordForwardToEndOfWord` (vim's `e`) fixed both
///    of those, but put `Right`'s cursor on a different landing grid
///    than `Left`'s (`MoveWordBackward`, start-of-word), so retracting
///    a forward selection with `Left` didn't return to where it grew
///    from.
/// 4. Keeping the cursor on `MoveWordForward`'s own grid but computing
///    the *visible* selection boundary separately (trailing the cursor
///    by one column whenever it sat right of the anchor) fixed both of
///    *those*, but broke something more fundamental: `edtui`'s own
///    cursor-cell paint (`EditorView::render`) runs *after* selection
///    styling and unconditionally repaints whatever cell `state.cursor`
///    is on, assuming throughout that it sits exactly on the
///    selection's own live end. Deliberately decoupling them left the
///    blinking terminal cursor visually sitting one column away from
///    the highlighted selection boundary.
/// 5. Accepting attempt 1's landing as "expected" (matching plain
///    character-wise `Shift+Right`'s own already-accepted "N+1, not N"
///    inclusive quirk) was tried next, on the theory that the real bug
///    was only the rendering mismatch from attempt 4 and that
///    `editor.rs::view`'s cursor-cell fix (painting the cursor's own
///    cell with `selection_style` whenever a selection is active, so
///    it's never visually out of sync with what `Copy` grabs -- see
///    that function's own comment) would resolve the rest. It did fix
///    the rendering-vs-copy mismatch (confirmed directly: real log
///    captures of `clipboard: set_text called` matched the logged
///    render pixel-for-pixel afterwards) -- but the underlying landing
///    itself was still wrong on its own terms: grabbing the first
///    character of the next word into the selection isn't a rendering
///    artifact to accept, it's simply not what was asked for.
/// 6. A hand-rolled scan (landing on a word's own last character *plus*
///    its trailing whitespace run, matching what "Draft "/"architecture "
///    looked like once copied and pasted for real) was tried next --
///    but that trailing-whitespace inclusion was never actually asked
///    for either, stated directly: selection should track only where
///    the cursor itself travels, nothing added on top. Confirmed
///    against a further real report -- `Ctrl+Shift+Right` from the
///    start of "the" landed one column past the 'e', on the space, not
///    on 'e' itself, which is exactly this same "extra character"
///    problem, just relocated to the opposite edge of the word instead
///    of eliminated.
///
/// **Landed on**: `MoveWordForwardToEndOfWord` (vim's `e`) for the
/// forward direction -- it already does exactly this, with no custom
/// scanning needed: self-skips whitespace, then lands on the *last*
/// character of a word, never on whitespace and never on the next
/// word's first character either. `Ctrl+Shift+Right` from the start of
/// "Draft architecture" now selects exactly "Draft" (cursor on the
/// second `'t'`), not "Draft " and not "Draft a". `state.cursor` and the
/// selection's own end are still kept exactly equal, always (attempt
/// 4's actual mistake, not the landing rule itself, so that invariant
/// stays intact) -- `MoveWordForwardToEndOfWord` already does this
/// itself, the same way `MoveWordBackward` does for the backward
/// direction below.
///
/// A real report also caught a *separate*, rendering-only bug once this
/// landing was correct: the real terminal's own bar-shaped cursor is
/// drawn at the *left* edge of whatever cell it's positioned on, so
/// sitting exactly on the last selected character's cell made the bar
/// visually read as the boundary *before* that character rather than
/// after it -- "the selection stopped one letter early" even though the
/// cell's own color and what `Copy` grabbed were both already correct.
/// See `Editor::cursor_screen_position`'s own doc comment for the fix
/// (shifts the *reported screen position* one column right while a
/// selection is active -- doesn't touch `state.cursor` or the selection
/// data itself, which were never the bug this time).
///
/// **Backward is still plain `MoveWordBackward`**, exactly as every
/// attempt above used it -- it was never the reported bug: it already
/// lands cleanly on a word's own first character with nothing extra
/// grabbed (confirmed by dedicated tests below, unchanged). `Right` and
/// `Left` land on two different grids now (`MoveWordForwardToEndOfWord`
/// stops at a word's *last* character, `MoveWordBackward` at its
/// *first*) -- attempt 3 tried this exact pairing and rejected it
/// specifically because `Right`-then-`Left` no longer lands back at an
/// identical column. That round-trip guarantee is deliberately not
/// pursued anymore: both directions are individually correct on their
/// own terms (cursor only ever visits real word boundaries, nothing
/// more), and forcing them onto a shared grid was what caused every
/// earlier attempt's actual bug in the first place.
///
/// **Seventh: a real report caught one more shape of this, specific to
/// *fresh* backward selections.** Plain (non-shifted) `Ctrl+Left`/`Right`
/// land the cursor on a word's own *first* character too (`bindings/mod.rs`'s
/// table, unmodified `MoveWordForward`/`MoveWordBackward`) -- so after a
/// plain `Ctrl+Right`, the cursor legitimately rests right at the start of
/// a word, e.g. the `'t'` of "the" in `"loaded the"`. Pressing
/// `Ctrl+Shift+Left` from there starts a fresh selection: `SwitchMode(Visual)`
/// anchors on that same cell (`'t'`), then `MoveWordBackward` -- vim's `b`,
/// already sitting at a word's own start -- jumps straight past it to the
/// *previous* word's start ("loaded"'s `'l'`). The anchor cell never
/// actually got *visited* by this selection (the cursor jumped clean over
/// it), but `edtui`'s inclusive-both-ends model keeps it in the range
/// anyway, so the highlighted (and copied) text came out `"loaded t"` --
/// reported directly, expected `"loaded "` instead, with the terminal
/// cursor rendered *before* the `'l'` it retracted to, not after it.
///
/// Fixed by trimming the anchor back one column, into the whitespace gap
/// it's actually sitting on the far edge of, whenever a fresh backward
/// selection's own anchor cell turns out to be a word's first character
/// (its left neighbor is whitespace) -- a single `state.lines.get` peek,
/// not a reimplementation of `edtui`'s own (unreachable, `pub(crate)`)
/// character classification: this only ever nudges an anchor that's
/// already sitting one cell past where it should be, never re-derives a
/// whole word span the way the deleted hand-rolled scans (attempt 6, and
/// its own less formal predecessors) tried and got wrong repeatedly.
/// Whitespace-only, not full word/punctuation-class boundaries -- the
/// reported case is a plain space between two words; a punctuation-class
/// boundary with no whitespace (e.g. landing exactly on a `':'` that
/// follows a word character) isn't covered by this and is left for a
/// future report if one ever surfaces, rather than reached for
/// speculatively. Guarded to the *first* press of a *fresh* selection
/// only (`state.mode != Visual` at entry) -- repeated presses extending an
/// already-open selection never touch the anchor at all, so there's
/// nothing to trim there.
pub(in crate::editor) fn extend_word_selection(state: &mut EditorState, forward: bool) {
    let cursor_before = state.cursor;
    let selection_before = state.selection.as_ref().map(|s| (s.start, s.end));
    let starting_fresh_selection = state.mode != EditorMode::Visual;

    if starting_fresh_selection {
        SwitchMode(EditorMode::Visual).execute(state);
    }

    if forward {
        MoveWordForwardToEndOfWord(1).execute(state);
    } else {
        MoveWordBackward(1).execute(state);
        trim_anchor_off_a_word_it_never_visited(state, cursor_before, starting_fresh_selection);
    }

    let selection_after = state.selection.as_ref().map(|s| (s.start, s.end));
    tracing::debug!(
        forward,
        ?cursor_before,
        ?selection_before,
        cursor_after = ?state.cursor,
        ?selection_after,
        "extend_word_selection"
    );
}

/// See `extend_word_selection`'s own doc comment ("Seventh") for the real
/// report this fixes. Only meaningful right after a fresh backward
/// `MoveWordBackward` (`fresh_selection` guards that; a continuing
/// selection's anchor is never touched) -- if `cursor_before` (the cell
/// `SwitchMode(Visual)` anchored on) sits at the very start of a word
/// (its own left neighbor is whitespace, or there's nothing to its left
/// at all), that anchor never got *visited* by this press -- `edtui`'s
/// `MoveWordBackward`, already at a word-start, jumps clean over it to
/// the *previous* word -- so it's trimmed one column left, into the gap
/// it's actually resting past the edge of, rather than left riding along
/// into the selection.
fn trim_anchor_off_a_word_it_never_visited(state: &mut EditorState, cursor_before: Index2, fresh_selection: bool) {
    if !fresh_selection || cursor_before.col == 0 {
        return;
    }

    let left_of_anchor = Index2 { row: cursor_before.row, col: cursor_before.col - 1 };
    let anchor_is_a_words_first_character = match state.lines.get(left_of_anchor) {
        Some(c) => c.is_whitespace(),
        None => true,
    };
    if !anchor_is_a_words_first_character {
        return;
    }

    if let Some(selection) = state.selection.as_mut() {
        if selection.start == cursor_before {
            selection.start.col -= 1;
        }
    }
}


#[cfg(test)]
mod tests {
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
        extend_word_selection(&mut state, true);
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
        extend_word_selection(&mut state, true);
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

        extend_word_selection(&mut state, true);
        let after_first = state.selection.as_ref().expect("should have started a selection").end;

        extend_word_selection(&mut state, true);
        let after_second = state.selection.as_ref().expect("should still have a selection").end;
        assert!(after_second.col > after_first.col, "second press should extend further, not stall: {after_first:?} -> {after_second:?}");

        extend_word_selection(&mut state, true);
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
        extend_word_selection(&mut state, false);
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

        extend_word_selection(&mut state, false);
        let after_first = state.selection.as_ref().expect("should have started a selection").end;

        extend_word_selection(&mut state, false);
        let after_second = state.selection.as_ref().expect("should still have a selection").end;
        assert!(after_second.col < after_first.col, "second press should extend further back, not stall: {after_first:?} -> {after_second:?}");

        extend_word_selection(&mut state, false);
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
        extend_word_selection(&mut state, false);
        let selection = state.selection.expect("should have started a selection");
        assert_eq!(selection.start.col, 5, "anchor should trim back into the space, not ride along on 'w'");
        assert_eq!(selection.end.col, 0, "should land on 'h', the start of \"hello\"");
        assert_eq!(state.cursor.col, 0);
    }

    /// The trim above must not fire for the ordinary case: starting a
    /// backward selection from a cursor that's genuinely *inside* a word
    /// (not at its start) should behave exactly as before -- the anchor
    /// legitimately belongs to the word being trimmed into, same as
    /// `ctrl_shift_left_selects_the_whole_previous_word_with_no_extra_character`
    /// above (which starts from an end-of-line boundary, a different but
    /// also-unaffected shape).
    #[test]
    fn ctrl_shift_left_from_mid_word_does_not_trim_the_anchor() {
        let mut state = state_for("hello world", 8); // cursor on 'r', mid-"world"
        extend_word_selection(&mut state, false);
        let selection = state.selection.expect("should have started a selection");
        assert_eq!(selection.start.col, 8, "anchor legitimately sat inside \"world\" -- must not be trimmed");
        assert_eq!(selection.end.col, 6, "should land on 'w', the start of \"world\"");
    }

    // No `Right`-then-`Left` round-trip test here anymore: forward now
    // lands on a word's own *last* character (see
    // `ctrl_shift_right_does_not_select_into_the_next_word` above) while
    // backward lands on a word's *first* character (`MoveWordBackward`,
    // unmodified, never broken) -- two genuinely different, individually-
    // correct landing conventions, not a shared grid anymore. See
    // `extend_word_selection`'s own doc comment for why exact symmetry
    // was given up rather than fixed.
}

#[cfg(test)]
mod word_selection_on_realistic_text {
    use edtui::{EditorMode, EditorState, Lines};

    use super::extend_word_selection;

    fn state_for(contents: &str, cursor_col: usize) -> EditorState {
        let mut state = EditorState::new(Lines::from(contents));
        state.mode = EditorMode::Insert;
        state.cursor.col = cursor_col;
        state
    }

    /// Real reported text (`"- App: owns panels, active index"`,
    /// architecture-doc-shaped) -- unlike every test above, this mixes
    /// in punctuation (`:`, `,`) between words, which `edtui`'s own
    /// 3-way character classification (word / punctuation / whitespace)
    /// treats as its own single-character "word". Repeated
    /// `Ctrl+Shift+Left` from the end of the line should still strictly
    /// monotonically extend the selection leftward, landing on each
    /// word *and* each punctuation run in turn, never stalling or
    /// jumping backward.
    #[test]
    fn repeated_left_monotonically_extends_through_punctuation() {
        let text = "- App: owns panels, active index";
        let mut state = state_for(text, text.chars().count());

        let mut previous = state.cursor.col;
        for _ in 0..8 {
            extend_word_selection(&mut state, false);
            let sel = state.selection.as_ref().expect("should have a selection");
            assert!(sel.end.col < previous, "should keep moving left, not stall or reverse: {previous} -> {}", sel.end.col);
            assert_eq!(sel.end, state.cursor, "left of the anchor, the selection end should exactly track the cursor");
            previous = sel.end.col;
            if previous == 0 {
                break;
            }
        }
    }

    /// The colon right after "App" is its own single-character
    /// "word" (punctuation class, distinct from the surrounding word
    /// characters) -- confirms it's a real, individually-selectable
    /// stop, not silently merged into "App" or into the following
    /// whitespace.
    #[test]
    fn colon_is_its_own_word_stop() {
        let text = "App: owns";
        let mut state = state_for(text, text.chars().count());

        extend_word_selection(&mut state, false); // "owns"
        extend_word_selection(&mut state, false); // ":"
        let sel = state.selection.as_ref().expect("should have a selection");
        assert_eq!(sel.end.col, 3, "should land on the ':' itself, index 3");
        assert_eq!(&text[3..4], ":");
    }

    /// Forward selection through punctuation must stop cleanly too --
    /// `"App:"` has no space between `"App"` and `':'`, so the first
    /// press lands right on the second `'p'`, not swallowing the `':'`
    /// into the same selection (`MoveWordForwardToEndOfWord` breaks on
    /// the class change, same as it does for whitespace). The very next
    /// press must still make progress onto the `':'` itself rather than
    /// stalling right at that boundary.
    #[test]
    fn repeated_right_extends_through_punctuation_with_no_extra_character() {
        let text = "App: owns panels";
        let mut state = state_for(text, 0);

        extend_word_selection(&mut state, true); // "App"
        let after_first = state.selection.as_ref().expect("should have a selection").end.col;
        assert_eq!(after_first, 2, "should land on the second 'p' of \"App\", not swallow the ':'");

        extend_word_selection(&mut state, true); // ":"
        let after_second = state.selection.as_ref().expect("should still have a selection").end.col;
        assert!(after_second > after_first, "second press should make progress onto the ':' itself: {after_first} -> {after_second}");
    }
}
