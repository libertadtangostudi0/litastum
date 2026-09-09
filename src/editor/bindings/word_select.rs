use edtui::actions::{Execute, MoveBackward, MoveWordBackward, MoveWordForward, MoveWordForwardToEndOfWord, SwitchMode};
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
/// **Twelfth: a real report on the *other* shape of anchor mis-trim --
/// the anchor sitting on whitespace itself, not on a word's own first
/// character.** Cursor placed right after the last letter of "derived"
/// in `"Draft architecture derived from the planning chat + the Far
/// Manager UI"` (i.e. resting on the space between "derived" and
/// "from" -- the ordinary place a cursor sits right after typing or
/// moving past a word), then `Ctrl+Shift+Left`: `SwitchMode(Visual)`
/// anchors on that space (`cursor_before`), `MoveWordBackward`
/// self-skips it and lands on "derived"'s own first letter -- exactly
/// right on its own -- but the anchor (the space) is still sitting at
/// the selection's *other* end, and `edtui`'s inclusive-both-ends model
/// keeps it in the range regardless, producing `"derived "` (with the
/// trailing space) instead of `"derived"`. This is the Seventh fix's
/// own blind spot from the opposite side: Seventh trims an anchor that
/// sits on a word's first character (motion jumps *over* it); this is
/// an anchor that sits on pure whitespace filler the cursor merely
/// happened to be resting on -- never real content the selection should
/// claim either, for the same underlying reason. Fixed by extending the
/// same trim to also fire when `cursor_before`'s own cell is whitespace
/// (not just when its left *neighbor* is) -- one more `state.lines.get`
/// peek, still no `CharacterClass` reimplementation. Confirmed against
/// the reported text: anchor trims from the space back onto "derived"'s
/// own last letter, giving exactly `"derived"`, no trailing space.
///
/// **Thirteenth: a real report showing Seventh and Twelfth were both
/// narrower special cases of one general rule, not two unrelated
/// fixes.** Cursor placed *mid-word*, inside plain "roadmap" (no
/// whitespace or word boundary anywhere nearby) -- between the `'d'`
/// and `'m'`, i.e. resting on `'m'` itself, exactly the shape
/// `ctrl_shift_left_from_mid_word_does_not_trim_the_anchor` (below)
/// had asserted was *already correct* to leave untrimmed. `Ctrl+Shift+
/// Left` from there selected `"roadm"` (the anchor's own `'m'` dragged
/// in), reported as wanting `"road"` instead. Once looked at directly
/// against the character-wise `Left` fix elsewhere in this same file
/// (`shift_select.rs::backward_anchor`, which *never* includes
/// `cursor_before`'s own cell for backward motion -- "the character to
/// select is always the one the cursor is about to move *onto*, never
/// the one it's currently sitting on"), it's clear Seventh and Twelfth
/// were only ever fixing two specific *symptoms* of the anchor keeping
/// `cursor_before`'s own real character at all, word-start and
/// whitespace being just the two shapes that happened to get reported
/// first. The actual rule needed no per-shape classification at all:
/// **any real character at all sitting under a fresh backward
/// selection's own anchor was never meant to be included**, because a
/// purely backward extension should only ever claim cells the motion
/// actually steps *onto*, mirroring `backward_anchor`'s own established
/// convention exactly. Simplified `trim_anchor_off_a_word_it_never_visited`
/// to that one check (`state.lines.get(cursor_before).is_some()`) --
/// Seventh's and Twelfth's own cases both still trim (a word's first
/// letter and a whitespace cell are each real characters), and the
/// previously-untested mid-word case now trims too, both without any
/// new classification logic. The one case that still legitimately
/// never trims is the append position past a line's own last character
/// (`ctrl_shift_left_selects_the_whole_previous_word_with_no_extra_character`
/// below) -- there's genuinely no character there, so `is_some()` is
/// already `false` on its own, no special-casing needed for it either.
///
/// **Fourteenth: a real report that the mirror-image direction had no
/// retracing logic of its own at all.** `"Draft architecture derived"`,
/// cursor placed right after "architecture" (i.e. on the space before
/// "derived"), then `Ctrl+Shift+Left` twice (selects "architecture",
/// then extends through "Draft" too, landing on `"Draft architecture"`)
/// followed by `Ctrl+Shift+Right` once: reported `"t architecture"`
/// instead of the wanted `"architecture"` (undoing exactly the second
/// `Left` press, same as `Left` undoing a `Right` already does via
/// `retract_onto_the_separator`/`retracing` above). Root cause: a
/// `Right` press on a selection `Editor::extend_word_selection` has
/// tagged `WordSelectTouch::NativeBackward` (built purely by walking
/// backward) always ran the *ordinary* forward branch --
/// `MoveWordForwardToEndOfWord` from wherever the cursor currently sits
/// -- with no awareness that this selection was built walking the
/// *other* way and that a `Right` here should retrace that walk, not
/// extend past it. From `"Draft"`'s own start (where the second `Left`
/// left the cursor), `MoveWordForwardToEndOfWord` lands on `'t'`,
/// `"Draft"`'s own *last* character -- nowhere close to undoing
/// anything.
///
/// **Landed on**: `Editor::extend_word_selection` now also recognizes
/// the mirror condition -- a continuing `forward` press against a
/// `NativeBackward`-tagged selection -- and passes it through the same
/// `retracing` parameter used for the opposite direction (the parameter
/// itself was renamed from `retracting` to reflect that it's no longer
/// backward-only). A `retracing` forward press calls
/// `retreat_forward_through_a_backward_walk` (below) instead of the
/// ordinary forward branch: plain `MoveWordForward` (the mirror of
/// `MoveWordBackward`, the action that built this walk, landing on
/// exactly the same word-start stops in reverse), closing the selection
/// entirely once that reaches or passes the anchor -- see that
/// function's own doc comment for why *reaches or passes*, not just
/// *reaches exactly*.
///
/// **Fifteenth: a real report exposing the simplest possible round trip
/// as broken -- one word extended forward, then immediately retracted.**
/// `"Draft architecture"`, cursor right before "architecture" (column
/// 6), `Ctrl+Shift+Right` (selects "architecture") then
/// `Ctrl+Shift+Left` once: reported `" "` (the single space before
/// "architecture") selected, wanted `""` -- nothing at all, landing
/// back exactly where the `Right` press started. Root cause: the
/// Eleventh fix's own "claims only new territory" step ran
/// unconditionally the moment `MoveWordBackward` landed exactly on the
/// anchor, without distinguishing "this coincidence is the very *first*
/// backward press against a freshly-built selection, so the anchor
/// really is the true start with nothing further to give back" from
/// "several presses have already retracted other words, and this
/// coincidence means only the earlier ones are gone, not that the
/// selection is done" -- mechanically, both look identical at this
/// point (`state.cursor == selection.start`), so the code always took
/// the same one further step past the anchor, whether or not there was
/// truly more to retract.
///
/// **Landed on**: stop taking that extra step at all. Once
/// `MoveWordBackward` lands exactly on the anchor, the entire
/// originally-forward-built selection has been given back in full --
/// close it immediately, the same as every other "nothing left
/// selected" case in this file, rather than treat the anchor's own far
/// side as unclaimed territory worth grabbing. This directly fixes the
/// new report (`Right` then `Left` now closes cleanly, matching a basic
/// round-trip guarantee), and turns out not to lose anything from the
/// *original* Eleventh report either: re-checked against its own
/// `"Draft architecture derived"` example (two `Right`s, then three
/// `Left`s) with this change applied, the second `Left` now closes the
/// selection outright instead of leaving `" "` selected -- but the
/// *third* `Left` then starts a perfectly ordinary *fresh* backward
/// selection from the same anchor position (mode having just returned
/// to `Insert`), and `trim_anchor_off_a_word_it_never_visited` (this
/// fresh press lands on a real character, "architecture"'s own first
/// letter) trims it exactly one column back into the gap -- landing on
/// `"Draft "` in full, byte-for-byte the same result Eleventh's own fix
/// produced with three continuing presses through a still-open
/// selection. The old "move the anchor to claim new territory" step was
/// never actually necessary to reach that result -- an ordinary fresh
/// selection re-derives the identical answer on its own.
pub(in crate::editor) fn extend_word_selection(state: &mut EditorState, forward: bool, retracing: bool, true_anchor: &mut Option<Index2>) {
    let cursor_before = state.cursor;
    let selection_before = state.selection.as_ref().map(|s| (s.start, s.end));
    let starting_fresh_selection = state.mode != EditorMode::Visual;

    if starting_fresh_selection {
        SwitchMode(EditorMode::Visual).execute(state);
    }

    if forward {
        if retracing {
            retreat_forward_through_a_backward_walk(state, true_anchor);
        } else {
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
        }
    } else {
        if starting_fresh_selection {
            *true_anchor = Some(cursor_before);
        }
        MoveWordBackward(1).execute(state);
        trim_anchor_off_a_word_it_never_visited(state, cursor_before, starting_fresh_selection);

        let landed_exactly_on_the_anchor = state.selection.as_ref().is_some_and(|s| s.start == state.cursor);
        if landed_exactly_on_the_anchor {
            // The whole selection has been fully given back -- nothing
            // of it is left to retract any further. `edtui`'s
            // inclusive-both-ends model can't represent an empty
            // selection as `Some` -- a single-cell `Selection` always
            // shows as one highlighted (and copyable) character -- so
            // the only way to show "nothing selected" is closing it
            // back to `Insert` entirely, same as `exit_selection()` in
            // `bindings/mod.rs`. See "Fifteenth" in this function's own
            // doc comment for why this used to instead take one more
            // step *past* the anchor and claim that as new territory --
            // reverted for breaking the basic "extend one word, then
            // immediately retract it" round trip.
            SwitchMode(EditorMode::Normal).execute(state);
            SwitchMode(EditorMode::Insert).execute(state);
        } else if retracing {
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

/// See `extend_word_selection`'s own doc comment ("Seventh," "Twelfth,"
/// and "Thirteenth" -- the general rule this landed on after two
/// narrower special cases) for the real reports this fixes. Only
/// meaningful right after a fresh backward `MoveWordBackward`
/// (`fresh_selection` guards that; a continuing selection's anchor is
/// never touched): trims the anchor (the cell `SwitchMode(Visual)`
/// anchored on, `cursor_before`) one column left, into the gap it's
/// actually resting past the edge of, whenever a real character sits
/// there at all -- mirroring `shift_select.rs::backward_anchor`'s own
/// already-established rule for plain character-wise `Left` ("the
/// character to select is always the one the cursor is about to move
/// *onto*, never the one it's currently sitting on"). No character
/// classification needed: a purely backward extension should never
/// claim the cell it started on, whatever that cell happens to hold.
fn trim_anchor_off_a_word_it_never_visited(state: &mut EditorState, cursor_before: Index2, fresh_selection: bool) {
    if !fresh_selection || cursor_before.col == 0 {
        return;
    }

    if state.lines.get(cursor_before).is_none() {
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
/// own `retracing` flag before this is ever called, not by anything
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

/// See `extend_word_selection`'s own doc comment ("Fourteenth") for the
/// real report this fixes. Called instead of the normal forward branch
/// whenever the caller's `retracing` flag says this `Right` press is
/// giving back territory a purely backward (`Ctrl+Shift+Left`) walk
/// built, rather than extending forward past the anchor into new text.
///
/// Uses plain `MoveWordForward` (lands on the *next* word's own first
/// character), not `MoveWordForwardToEndOfWord` (the normal forward
/// branch's own action, which lands on a word's *last* character) --
/// this is the exact mirror of `MoveWordBackward`, the action that
/// built this walk in the first place, so retracing with it lands back
/// on precisely the same stops the backward walk itself created,
/// undoing one step exactly. No extra separator step is needed here,
/// unlike `retract_onto_the_separator` above: `MoveWordBackward` always
/// jumps clean through a separator in one atomic move when building a
/// backward walk (never stopping *on* it), so the plain mirror already
/// re-crosses that same gap in one step too -- there's no leftover gap
/// this side ever needs a second nudge for, unlike a *forward*-built
/// selection's own trailing gap (`MoveWordForwardToEndOfWord` stops
/// short of it, which is what made `retract_onto_the_separator`
/// necessary in the first place).
///
/// `>=`, not `==`, against the anchor (`selection.start`): a
/// backward walk's own fresh press trims the anchor one column *past*
/// the last real landing spot `MoveWordBackward` used (see
/// `trim_anchor_off_a_word_it_never_visited`), which is never itself a
/// stop on `MoveWordForward`'s own landing grid -- so the very last
/// retracing press does not land exactly on the anchor, it *overshoots*
/// past it into whatever word comes after. Comparing in reading order
/// (row, then column) catches that overshoot the same way an exact
/// match would catch a clean landing, and either way means the same
/// thing: nothing backward-built is left to give back, so the
/// selection closes entirely, snapped back to the anchor itself rather
/// than left sitting past it. Deliberately does not try to keep going
/// past the anchor into a fresh forward extension the way the backward
/// side's own "claims only new territory" fix does (`extend_word_selection`'s
/// "Eleventh") -- not part of the report this fixes; left for a future
/// report if continuing to extend forward past a fully-retraced
/// backward walk ever turns out to be wanted.
///
/// **Sixteenth: a real report that closing landed one column short of
/// where the backward walk actually started.** `"derived"`, cursor
/// between `'i'` and `'v'` (column 4): `Ctrl+Shift+Left` selects
/// `"deri"` (the fresh press trims the anchor from 4 to 3 -- see
/// `trim_anchor_off_a_word_it_never_visited`), then `Ctrl+Shift+Right`
/// retraces it -- reported landing between `'r'` and `'i'` (column 3)
/// instead of back at column 4, where the whole thing actually started.
/// Root cause: closing here used to snap `state.cursor` to
/// `selection.start` directly, which *is* the anchor `MoveWordBackward`
/// itself used to build the walk, but it's the **trimmed** value, one
/// column short of the real starting point by design (the whole point
/// of the trim is excluding that one column from the *visible*
/// selection while it's open) -- using it again to decide where
/// *closing* lands reintroduces the exact same one-column error the
/// trim exists to fix in the other direction. Fixed by reading back
/// `true_anchor` (`Editor::word_select_true_anchor`) instead --
/// captured by the caller at the exact moment the fresh backward press
/// opened this walk, before any trim ever touched anything, so it's
/// always the real, untrimmed starting column. Falls back to `anchor`
/// itself only if nothing was tracked (defensive; every backward-built
/// walk this function ever retraces was necessarily opened by a fresh
/// press that also sets `true_anchor` in the same breath).
///
/// This also happens to be exactly the state VS Code's own
/// `Ctrl+Shift+Right` reaches after undoing a `Ctrl+Shift+Left`
/// selection on the same word (confirmed directly against it): once
/// back at the true original column, a *further* `Ctrl+Shift+Right`
/// starts a perfectly ordinary fresh *forward* selection from there
/// (this function doesn't need to do anything more for that --
/// `extend_word_selection`'s own fresh-forward branch already handles
/// it once `state.mode` is back to `Insert`) -- landing on `"ved"` for
/// this same "derived" example, matching VS Code's own "reflects"
/// behavior once mirrored through one extra `Right` press instead of
/// VS Code's single one (this codebase's own `retracing` step and a
/// fresh extension are two separate presses, not one combined action).
fn retreat_forward_through_a_backward_walk(state: &mut EditorState, true_anchor: &mut Option<Index2>) {
    MoveWordForward(1).execute(state);

    let Some(anchor) = state.selection.as_ref().map(|s| s.start) else {
        return;
    };
    let reached_or_passed_the_anchor = (state.cursor.row, state.cursor.col) >= (anchor.row, anchor.col);
    if !reached_or_passed_the_anchor {
        return;
    }

    let restore_to = true_anchor.take().unwrap_or(anchor);
    state.cursor = restore_to;
    if let Some(selection) = state.selection.as_mut() {
        selection.end = restore_to;
    }
    SwitchMode(EditorMode::Normal).execute(state);
    SwitchMode(EditorMode::Insert).execute(state);
}


#[cfg(test)]
mod tests;
#[cfg(test)]
mod word_selection_on_realistic_text;
