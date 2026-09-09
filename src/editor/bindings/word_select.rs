use edtui::actions::{Execute, MoveBackward, MoveWordBackward, MoveWordForwardToEndOfWord, SwitchMode};
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
///
/// **Eighth: the mirror-image report, on *retracting* an already-open
/// selection.** Extending forward always lands on a word's own *last*
/// character (see "landed on" above); retracting one press with `Left`
/// from exactly that cell is `MoveWordBackward`'s other case -- not
/// already at the word's own *start*, so vim's `b` lands there instead,
/// same word, just its opposite end. For a *fresh* selection that's
/// already correct and already tested (`ctrl_shift_left_selects_the_
/// whole_previous_word_with_no_extra_character` below) -- landing on a
/// word's own start when extending backward from its own last character
/// is exactly "select this whole word." But when *retracting* an
/// existing forward-built selection, the cell this lands on is still
/// *inside* the word that one `Ctrl+Shift+Right` press just added --
/// reported directly against real text: a full-line selection ending in
/// "...2-column panels", retracted once, kept the `'p'` of "panels"
/// selected (`"...2-column p"`) instead of removing "panels" entirely.
///
/// **First attempt** skipped all the way back to the *previous* word's
/// own last character (mirroring the anchor trim's own "land exactly
/// where the opposite-direction press would have" instinct) -- landing
/// back on "column"'s own `'n'`, i.e. `"...2-column"` with no trailing
/// space. **Also wrong**, reported directly right after: the wanted
/// result keeps the separating space, `"...2-column "` -- retracting a
/// word should remove *that word's own text* and land right on the gap
/// in front of it, not additionally swallow the gap too. This is
/// actually the same convention the anchor trim above already
/// settled on (trimming *into* the gap, not past it) -- this fix
/// originally didn't mirror that closely enough.
///
/// **Second attempt** landed on the gap correctly, but decided *whether*
/// to apply it by peeking at `cursor_before`'s own character: a word
/// character with a non-word character (or nothing) right after it (the
/// shape a `Right` press leaves behind), or already whitespace (the
/// shape *this fix's own correction* leaves behind, needed for repeated
/// `Left` presses to keep cascading). **Wrong a third way**, reported
/// directly against real text with trailing punctuation: selecting
/// through `"...2-column panels,"` (comma included) and retracting once
/// landed on `'p'` again -- `cursor_before` was the comma itself, which
/// is neither a word character nor whitespace, so the guard's `_ =>
/// false` catch-all silently skipped the correction. Widening the guard
/// to *also* treat punctuation as "complete" was tried and rejected
/// before it was even applied: `edtui`'s own word-backward motion stops
/// on a lone punctuation character the same way it stops on a word (see
/// `colon_is_its_own_word_stop` below), so a *pure* backward-only
/// selection walking through real punctuation runs (`repeated_left_
/// monotonically_extends_through_punctuation` below) would have started
/// hitting this same "step into the gap" correction too -- something
/// that selection never asked for and was never broken, confirmed by
/// tracing it by hand rather than just changing the guard and hoping.
/// The actual problem was deeper: no reading of `cursor_before`'s own
/// character can *reliably* distinguish "this cell is where a `Right`
/// press left the cursor" from "this cell is just where a pure backward
/// walk happens to be passing through" -- both can land on the exact
/// same kind of cell (a word's last letter, a lone punctuation mark, or
/// this fix's own leftover whitespace).
///
/// A fourth issue turned up before this even shipped, thinking through
/// it further: the punctuation report's *own* selection ("...2-column
/// panels,") was never actually built by repeated `Ctrl+Shift+Right`
/// presses at all in the first place -- more likely a whole line
/// selected some other way (character-wise, a mouse drag, ...) and then
/// trimmed with `Ctrl+Shift+Left`. A simple "has `Right` ever fired"
/// flag would stay `false` for that the whole time (no `Right` press
/// ever happens), so it would have kept missing this exact report even
/// once fixed for the comma.
///
/// **Landed on**: stop guessing from characters entirely for the
/// *decision* of whether to apply the correction, and use real state
/// instead -- but a plain "has gone forward" boolean isn't enough
/// state, per the paragraph just above. `Editor::extend_word_selection`
/// (`editor.rs`) tracks a real three-state `WordSelectTouch` instead
/// (see its own doc comment for the full reasoning): whether the
/// current selection is untouched by word-wise selection (built some
/// other way, or this is its first backward touch), a pure backward
/// walk word-wise selection built entirely itself, or has had at least
/// one forward press or retraction. `retracting` -- this function's own
/// parameter -- collapses that down to the one bit this function
/// actually needs: retract fully unless the selection is a pure
/// backward walk. The character peek left in this file
/// (`retract_onto_the_separator`) now only ever answers the purely
/// mechanical question "is there actually a gap right before wherever
/// `MoveWordBackward` landed" -- never "should this correction apply at
/// all," which is what kept going wrong across every earlier attempt
/// above.
///
/// **Ninth: a real report caught the mechanical half being too narrow,
/// not the decision half.** `retract_onto_the_separating_space` (this
/// function's original name) only stepped onto the gap when it was
/// whitespace -- reported directly against `"...arrow-key"`: retracting
/// "key" landed on `'k'` (`"...arrow-k"`), one column short of the
/// wanted `"...arrow-"`, because the character right before `'k'` is
/// `'-'`, punctuation, not whitespace, so the whitespace-only check
/// silently declined to take the extra step. But `MoveWordBackward`
/// (already run by the caller, unconditionally, before this ever looks
/// at anything) *always* lands at the start of a same-class character
/// run -- that's what "word motion" means -- so whatever sits
/// immediately to its left can never be more of the *same* word; it's
/// always either nothing (start of line) or a genuine class boundary,
/// whitespace or punctuation alike. The whitespace-only check was
/// therefore never actually narrowing to a *safer* case, just an
/// *incomplete* one -- renamed to `retract_onto_the_separator` and
/// broadened to step back onto whatever character is there, with no
/// classification at all: existence of a left neighbor is already the
/// whole answer.
///
/// **Tenth: a real report on the opposite edge -- starting a *fresh*
/// selection right where there's nowhere left to go at all.** Cursor at
/// the very start of the buffer (column 0, nothing before it), pressed
/// `Ctrl+Shift+Left`: `SwitchMode(Visual)` anchors the selection on that
/// same cell (column 0, same as every other fresh selection), then
/// `MoveWordBackward` -- already at the very start, nothing to jump to
/// -- clamps and leaves the cursor exactly where it was. `edtui`'s
/// inclusive-both-ends selection then covers that one, single,
/// never-actually-moved-to cell -- `"Draft architecture"` reported one
/// character selected (`"D"`) after two `Ctrl+Shift+Left` presses from
/// the start of the line, wanted none at all.
///
/// **First attempt** compared `state.cursor` against `cursor_before`
/// after a *freshly opened* selection's own first motion, and closed
/// the selection back down whenever that motion made zero progress --
/// direction-agnostic (checked after both branches, not a
/// backward-only special case), on the theory that the identical trap
/// exists for a fresh `Ctrl+Shift+Right` at the buffer's own end too.
/// **Still wrong, reported again on a retest**: fixed the two-press
/// case from the original report (both presses now genuinely no-ops),
/// but missed a *different* path to the exact same "D" -- retracting an
/// *existing* forward-built selection (`Ctrl+Shift+Right` then
/// `Ctrl+Shift+Left`, not two backward presses) back down past its own
/// first and only word. There, `starting_fresh_selection` is `false`
/// (the selection was already open) and `MoveWordBackward` genuinely
/// *does* move (from "Draft"'s own last character back to its first) --
/// the zero-progress check never even looks at this case, but the
/// result is the identical phantom: the cursor lands exactly back on
/// `selection.start` (the anchor `Ctrl+Shift+Right` planted at column 0
/// in the first place), and `edtui`'s inclusive model still shows that
/// coincidence as one selected character instead of none.
///
/// **Landed on**: the two reports are the same underlying shape wearing
/// different clothes -- in both, retracting (or a fresh selection's
/// very first motion, which is retracting-from-nothing in the same
/// sense) ends with the cursor exactly on the selection's own anchor,
/// meaning nothing of substance remains between them. Checking
/// `cursor_before` was only ever a proxy for that, and an incomplete
/// one (it only catches the *zero-motion* route to the coincidence, not
/// every route). Checking `state.cursor == selection.start` directly
/// catches every way of arriving there -- zero motion at the buffer's
/// edge, or real motion that still lands squarely back on where the
/// selection began.
///
/// **Eleventh: a real report on what happens on the *next* press after
/// that.** The ninth's own `"...arrow-key"` case, and the ordinary
/// `"hello world foo"` case, both retract a word that was never the
/// selection's own anchor -- there's always more selected text further
/// right, so landing on (or past) the anchor never came up. But
/// starting the selection *mid-buffer* (`Ctrl+Shift+Right` twice from
/// right before `"architecture"` in `"Draft architecture derived"`,
/// selecting `"architecture derived"`) and then retracting *past* both
/// words hits exactly that: the second `Ctrl+Shift+Left` retracts
/// `"derived"` normally (landing on the space after `"architecture"`,
/// same as every other case), but the *third* press's `MoveWordBackward`
/// lands exactly on the anchor (`"architecture"`'s own first letter,
/// where `Ctrl+Shift+Right` first anchored) -- the tenth fix's own
/// anchor-coincidence check would apply here too, except
/// `retract_onto_the_separator` still runs its own extra step *after*
/// that check would have looked, landing one column *further left*, on
/// the space *before* `"architecture"` -- genuine, real territory that
/// was never part of the selection at all, since the selection started
/// exactly at the anchor and never extended left of it. `state.selection.start`
/// stayed put at the old anchor throughout, so the result covered both
/// that space *and* the anchor's own first letter (`" a"`) instead of
/// just the space (`" "`) -- reported directly, with a second data point
/// (one further `Ctrl+Shift+Left` past that) confirming the next press
/// then needs to walk on into `"Draft"` treating the space as its own
/// new anchor (`"Draft "`, not `"raft "` or anything else that would
/// result from *re*-trimming an already-settled anchor).
///
/// **Landed on**: when `MoveWordBackward` lands exactly on the anchor,
/// this is the exact moment the whole originally-forward-built selection
/// has been fully consumed -- whatever `retract_onto_the_separator` does
/// *next* is no longer "trimming the retracted word's own leading gap"
/// (there's no more retracted-word territory left to speak of), it's
/// staking out *brand new* territory the selection never covered before.
/// So the anchor needs to move with it, exactly once: if the separator
/// step makes real progress from here, `selection.start` is reset to
/// match the new cursor too (the anchor is now *this* cell, a fresh
/// single-character foothold in new territory -- explains `"Draft
/// architecture derived"`'s own `" "` result); if it can't make any
/// progress at all (nothing left, e.g. the buffer's own start, or
/// `retracting` was false to begin with, e.g. the tenth bug's own
/// literally-fresh-at-column-0 case), the selection is closed entirely
/// instead, same as before. Either way, this only ever happens *once* --
/// the moment the anchor moves (or the selection closes) is also the
/// moment `landed_on_anchor` stops being true on the *next* press,
/// because by then `cursor_before` no longer coincides with the old,
/// pre-move anchor at all (it's wherever this press just left it) --
/// so a further press proceeds as an entirely ordinary continuing
/// selection, extending only the moving end, exactly like any other
/// (explains the `"Draft "` result one press later: the fourth press's
/// own `MoveWordBackward` from the freshly-anchored space genuinely
/// walks into `"Draft"`, and finding nothing further to retract onto at
/// column 0 just leaves the anchor where the previous press already put
/// it).
pub(in crate::editor) fn extend_word_selection(state: &mut EditorState, forward: bool, retracting: bool) {
    let cursor_before = state.cursor;
    let selection_before = state.selection.as_ref().map(|s| (s.start, s.end));
    let starting_fresh_selection = state.mode != EditorMode::Visual;

    if starting_fresh_selection {
        SwitchMode(EditorMode::Visual).execute(state);
    }

    if forward {
        MoveWordForwardToEndOfWord(1).execute(state);
        if starting_fresh_selection && state.cursor == cursor_before {
            // Nowhere further right to go at all (a fresh selection
            // right at the buffer's own end) -- close it rather than
            // leave a phantom single-character selection sitting on the
            // cell it merely anchored on. No `retract`-style extra step
            // exists on this side to possibly still make progress, so
            // this is the whole check, unlike the backward branch below.
            SwitchMode(EditorMode::Normal).execute(state);
            SwitchMode(EditorMode::Insert).execute(state);
        }
    } else {
        MoveWordBackward(1).execute(state);
        trim_anchor_off_a_word_it_never_visited(state, cursor_before, starting_fresh_selection);

        let landed_exactly_on_the_anchor = state.selection.as_ref().is_some_and(|s| s.start == state.cursor);
        if landed_exactly_on_the_anchor {
            let cursor_before_the_separator_step = state.cursor;
            if retracting {
                retract_onto_the_separator(state);
            }
            if state.cursor == cursor_before_the_separator_step {
                // Nothing left to retract onto either -- the selection
                // has been fully consumed back to (or never moved past)
                // its own anchor, with no further territory to claim.
                // `edtui`'s inclusive-both-ends model can't represent an
                // empty selection as `Some` -- a single-cell `Selection`
                // always shows as one highlighted (and copyable)
                // character -- so the only way to show "nothing
                // selected" is closing it back to `Insert` entirely,
                // same as `exit_selection()` in `bindings/mod.rs`.
                SwitchMode(EditorMode::Normal).execute(state);
                SwitchMode(EditorMode::Insert).execute(state);
            } else if let Some(selection) = state.selection.as_mut() {
                // The separator step found genuine new territory beyond
                // the old anchor -- move the anchor to match, so the
                // selection reflects only that freshly-claimed cell
                // rather than stale ground stretching back to the word
                // that's already been fully retracted away.
                selection.start = state.cursor;
            }
        } else if retracting {
            retract_onto_the_separator(state);
        }
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

/// See `extend_word_selection`'s own doc comment ("Eighth," in
/// particular its "second attempt" and "landed on" paragraphs, and
/// "Ninth" for why this no longer checks *what kind* of character the
/// gap is) for why this only handles the purely mechanical half of the
/// fix now -- *whether* to apply it is decided entirely by the caller's
/// own `retracting` flag before this is ever called, not by anything
/// read here. All this does: if there's any cell at all right before
/// wherever `MoveWordBackward` (already run by the caller) landed, one
/// more plain `MoveBackward` (not another word motion) lands squarely
/// on it -- removing exactly the retracted word's own text and stopping
/// right at the separator in front of it, whitespace or punctuation
/// alike. No character classification needed: `MoveWordBackward` only
/// ever stops at the start of a same-class run, so whatever sits
/// immediately to its left can never be more of the word just
/// retracted -- it's always either a genuine separator or nothing
/// (start of line, the `col == 0` guard below).
fn retract_onto_the_separator(state: &mut EditorState) {
    if state.cursor.col == 0 {
        return;
    }

    let left_of_landing = Index2 { row: state.cursor.row, col: state.cursor.col - 1 };
    if state.lines.get(left_of_landing).is_some() {
        MoveBackward(1).execute(state);
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
        extend_word_selection(&mut state, true, false);
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
        extend_word_selection(&mut state, true, false);
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

        extend_word_selection(&mut state, true, false);
        let after_first = state.selection.as_ref().expect("should have started a selection").end;

        extend_word_selection(&mut state, true, false);
        let after_second = state.selection.as_ref().expect("should still have a selection").end;
        assert!(after_second.col > after_first.col, "second press should extend further, not stall: {after_first:?} -> {after_second:?}");

        extend_word_selection(&mut state, true, false);
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
        extend_word_selection(&mut state, false, false);
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

        extend_word_selection(&mut state, false, false);
        let after_first = state.selection.as_ref().expect("should have started a selection").end;

        extend_word_selection(&mut state, false, false);
        let after_second = state.selection.as_ref().expect("should still have a selection").end;
        assert!(after_second.col < after_first.col, "second press should extend further back, not stall: {after_first:?} -> {after_second:?}");

        extend_word_selection(&mut state, false, false);
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
        extend_word_selection(&mut state, false, false);
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
        extend_word_selection(&mut state, false, false);
        let selection = state.selection.expect("should have started a selection");
        assert_eq!(selection.start.col, 8, "anchor legitimately sat inside \"world\" -- must not be trimmed");
        assert_eq!(selection.end.col, 6, "should land on 'w', the start of \"world\"");
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

        extend_word_selection(&mut state, true, false); // "hello"
        extend_word_selection(&mut state, true, false); // "hello world"
        extend_word_selection(&mut state, true, false); // "hello world foo"

        extend_word_selection(&mut state, false, true); // retract "foo" -- `retracting=true`, simulating what `Editor::extend_word_selection` computes after those three `Right` presses

        assert_eq!(state.cursor.col, 11, "should land on the space right after \"world\" -- \"hello world \", not \"hello world\" or \"hello world f\"");
        let selection = state.selection.expect("should still have a selection");
        assert_eq!(selection.end, state.cursor, "the selection's own end must always equal the cursor, same invariant as everywhere else");
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

        extend_word_selection(&mut state, true, false); // "hello"
        extend_word_selection(&mut state, true, false); // "hello world"
        extend_word_selection(&mut state, true, false); // "hello world foo"

        // `retracting=true` on both -- `Editor::extend_word_selection` never
        // clears the flag on a backward press, only a fresh selection or a
        // `Right` press does, so an entire streak of `Left` presses after
        // one-or-more `Right`s all see it `true`, not just the first.
        extend_word_selection(&mut state, false, true); // retract "foo" -> "hello world "
        extend_word_selection(&mut state, false, true); // retract "world " -> "hello "

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
        extend_word_selection(&mut state, false, true); // retracting=true -- as if Editor found this selection's touch to be Untouched
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
        extend_word_selection(&mut state, false, true);
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

        extend_word_selection(&mut state, false, false);
        assert_eq!(state.mode, EditorMode::Insert, "should not have opened a selection with nowhere to go");
        assert!(state.selection.is_none(), "should not have selected the first character just by anchoring on it");

        extend_word_selection(&mut state, false, false);
        assert_eq!(state.mode, EditorMode::Insert, "a second press should hit the same wall, not accumulate a selection");
        assert!(state.selection.is_none());
    }

    /// Same shape, different real text -- pinned down independently
    /// since the first report's own retest happened to reuse this exact
    /// second string.
    #[test]
    fn ctrl_shift_left_at_the_very_start_of_the_buffer_selects_nothing_on_other_text_too() {
        let mut state = state_for("current scaffold", 0);

        extend_word_selection(&mut state, false, false);
        extend_word_selection(&mut state, false, false);

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

        extend_word_selection(&mut state, true, false); // Ctrl+Shift+Right, selects "Draft"
        assert!(state.selection.is_some(), "sanity check -- should have a real selection to retract");

        extend_word_selection(&mut state, false, true); // Ctrl+Shift+Left, retracts "Draft" -- retracting=true, as `Editor` computes after a forward press

        assert_eq!(state.mode, EditorMode::Insert, "retracting the only word back to its own start should close the selection, not leave \"D\" behind");
        assert!(state.selection.is_none());
    }

    /// Same shape as the test above, different real text -- matches the
    /// report's own second example verbatim.
    #[test]
    fn ctrl_shift_left_retracting_the_first_word_of_the_buffer_selects_nothing_on_other_text_too() {
        let mut state = state_for("current scaffold", 0);

        extend_word_selection(&mut state, true, false); // "current"
        extend_word_selection(&mut state, false, true); // retracts it fully

        assert_eq!(state.mode, EditorMode::Insert);
        assert!(state.selection.is_none(), "\"c\" must not end up selected after retracting the whole first word");
    }

    /// Regression test for the real, eleventh-attempt report: retracting
    /// *past* a mid-buffer selection's own anchor must claim only the
    /// genuinely new territory beyond it, not drag the anchor's own word
    /// along as stale leftover selection. `"Draft architecture derived"`,
    /// cursor placed right before the `'a'` of "architecture" (column 6):
    /// two `Ctrl+Shift+Right` presses select `"architecture derived"`;
    /// two `Ctrl+Shift+Left` presses should retract both words fully and
    /// land on the one separator beyond them (the space right after
    /// "Draft") -- `" "` alone, not `" a"` (the old bug: the anchor,
    /// still pinned at column 6, dragged "architecture"'s own first
    /// letter back into the selection even though the word itself was
    /// long gone).
    #[test]
    fn ctrl_shift_left_retracting_past_a_mid_buffer_anchor_claims_only_new_territory() {
        let mut state = state_for("Draft architecture derived", 6);

        extend_word_selection(&mut state, true, false); // "architecture"
        extend_word_selection(&mut state, true, false); // "architecture derived"

        extend_word_selection(&mut state, false, true); // retract "derived"
        extend_word_selection(&mut state, false, true); // retract "architecture", crossing the anchor

        let selection = state.selection.expect("should still have a selection -- the space beyond the anchor");
        assert_eq!(selection.start, state.cursor, "landing past the anchor should move the anchor to match, not leave it behind");
        assert_eq!(state.cursor.col, 5, "should land on the space right after \"Draft\" -- \" \", not \" a\"");
    }

    /// One further `Ctrl+Shift+Left` past the crossing above must behave
    /// as an entirely ordinary continuing selection from the
    /// newly-settled anchor -- walking into "Draft" and landing at
    /// column 0 (nothing left to retract onto there), giving `"Draft "`
    /// in full. Pinned down separately from the test above since this
    /// is the report's *other* data point (one more press than the
    /// first), not just a longer run of the same assertions.
    #[test]
    fn ctrl_shift_left_one_more_press_past_the_crossing_extends_normally() {
        let mut state = state_for("Draft architecture derived", 6);

        extend_word_selection(&mut state, true, false); // "architecture"
        extend_word_selection(&mut state, true, false); // "architecture derived"
        extend_word_selection(&mut state, false, true); // retract "derived"
        extend_word_selection(&mut state, false, true); // retract "architecture", crossing the anchor -- " "

        extend_word_selection(&mut state, false, true); // one more Left

        let selection = state.selection.expect("should still have a selection");
        assert_eq!(state.cursor.col, 0, "should land at the very start of \"Draft\"");
        assert_eq!(selection.start.col, 5, "the anchor settled by the crossing above should stay right where it was, not get re-trimmed");
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

        extend_word_selection(&mut state, false, false); // "web"
        let after_first = state.selection.as_ref().expect("should have a selection").end.col;
        extend_word_selection(&mut state, false, false); // "wide web"
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
            extend_word_selection(&mut state, false, false);
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

        extend_word_selection(&mut state, false, false); // "owns"
        extend_word_selection(&mut state, false, false); // ":"
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

        extend_word_selection(&mut state, true, false); // "App"
        let after_first = state.selection.as_ref().expect("should have a selection").end.col;
        assert_eq!(after_first, 2, "should land on the second 'p' of \"App\", not swallow the ':'");

        extend_word_selection(&mut state, true, false); // ":"
        let after_second = state.selection.as_ref().expect("should still have a selection").end.col;
        assert!(after_second > after_first, "second press should make progress onto the ':' itself: {after_first} -> {after_second}");
    }
}
