# Word-wise selection (`Ctrl+Shift+Left`/`Right`) -- history

Code: `src/editor/bindings/word_select.rs` (`extend_word_selection` and
its helpers), state in `Editor::word_select_touch` /
`Editor::word_select_true_anchor`, called from
`editor_keymap::handle_editor_key`. Seventeen real reports went into
the current behavior; this is the record of what was tried, what each
attempt broke, and why the current approach won. Read it before
changing anything in that file -- most "obvious" fixes are already in
here, reverted.

## Current behavior, in short

- **Forward** lands on the *last* character of the next word
  (`edtui`'s `MoveWordForwardToEndOfWord`, vim's `e`), never on
  whitespace and never on the next word's first character.
- **Backward** lands on a word's *first* character (`MoveWordBackward`,
  vim's `b`).
- The two directions land on different grids on purpose -- a
  `Right`-then-`Left` round trip landing on the identical column is
  *not* pursued (see attempts 3-6).
- `state.cursor` and the selection's own end are always kept exactly
  equal -- `edtui` assumes that invariant throughout (attempt 4).
- A fresh backward selection never includes the cell it started on
  (the anchor is trimmed one column back whenever a real character
  sits there) -- the same rule plain character-wise `Shift+Left`
  follows (`shift_select.rs::backward_anchor`).
- Retracting only happens when the selection's own build direction
  says so (`WordSelectTouch`), never guessed from characters.
- Whenever the cursor lands exactly on the anchor, the selection closes
  (`edtui`'s inclusive model can't show an empty selection).
- A non-ASCII run (em dash, curly quote, Cyrillic, ...) is walked by
  hand, because `edtui`'s own forward word motion is a no-op on it.

## Why this isn't a table entry

Every other binding is a declarative `Action` in
`bindings::standard_key_handler`. No fixed sequence of `edtui` actions
gives "repeated presses keep progressing," "never grabs an extra
character," and "the result matches where the cursor travelled" at
once, and `edtui`'s own `Selection`/`CharacterClass` types are
`pub(crate)` -- readable through `EditorState::selection`, but not
constructible, and its word-boundary rules can't be reused or safely
re-derived. So this is intercepted one layer up, in `editor_keymap`,
ahead of `Editor::input`.

## Attempts

Code comments and tests elsewhere refer to these by ordinal ("Eighth",
"Seventeenth") -- same numbers as below.

1. **Plain `MoveWordForward`/`MoveWordBackward`.** Lands on the first
   character of the adjacent word; `edtui`'s selection is inclusive on
   the cursor end, so extending `Right` grabbed that character into
   the highlight *and* into `Copy` ("planning chat " copied as
   "planning chat +").
2. **`MoveWordForward` + a corrective `MoveBackward(1)`.** Fixed that,
   but left the cursor mid-whitespace, so the next press's own word
   scan re-crossed the same gap and cancelled itself -- repeated
   presses silently did nothing.
3. **`MoveWordForwardToEndOfWord` (vim's `e`).** Fixed both, but put
   `Right` on a different landing grid than `Left`, so retracting a
   forward selection didn't return to where it grew from. Rejected at
   the time for that round-trip reason -- later adopted anyway (6).
4. **Cursor on `MoveWordForward`'s grid, visible selection end computed
   separately** (trailing the cursor by one column right of the
   anchor). Fixed all three properties -- and was reverted: `edtui`
   (its rendering, `cursor_screen_position`, ...) assumes the cursor
   sits exactly on the selection's live end. Decoupling them left the
   terminal cursor visibly one column away from the highlight, which
   read as broken rendering and as broken copy.
   **Lesson**: when a library's state has an undocumented cross-field
   invariant that several unrelated parts of it rely on, treat it as
   load-bearing even where it's inconvenient.
5. **Accept attempt 1's landing as expected** (like character-wise
   `Shift+Right`'s "N+1, not N"), on the theory that only rendering was
   wrong. Fixing the render (`Editor::view` paints the cursor cell in
   `selection_style` during a selection) made render and copy agree --
   confirmed from logs -- but then the actual complaint was visible:
   grabbing the next word's first letter was simply not what was
   wanted.
6. **Hand-rolled scan: a word's last character plus its trailing
   whitespace.** Also not wanted: the selection should track only
   where the cursor travels, nothing added on either side. Deleted.
   **Landed on attempt 3's action after all** -- by then the
   round-trip guarantee that got it rejected had been given up, and it
   lands exactly on a word's own boundaries. A separate render-only
   bug followed: the terminal's bar cursor is drawn at the *left* edge
   of its cell, so sitting on the last selected character read as
   "stopped one letter early" -- fixed in `Editor::cursor_screen_position`
   (shift the reported position right while extending forward), not
   in the selection data.
7. **A fresh backward selection from a word's first character**
   (where a plain `Ctrl+Right` leaves the cursor) kept that character:
   `SwitchMode(Visual)` anchors on it, `MoveWordBackward` jumps clean
   past it, the inclusive model keeps it ("loaded t" instead of
   "loaded "). Trimmed the anchor one column back into the gap.
8. **Retracting a forward-built selection with `Left`** landed on the
   retracted word's own first letter (`MoveWordBackward` from a word's
   last character stops at the same word's start) -- "...2-column p".
   First fix skipped past the space too ("...2-column", reported
   wrong: the space must stay); second landed on the gap but decided
   *whether* to by peeking at the previous character, which failed on
   trailing punctuation ("panels," -- the comma is neither word nor
   whitespace). No character peek can tell "a `Right` press left the
   cursor here" from "a backward walk passes through here," and a
   selection might not have been built by `Right` presses at all (a
   drag, a character-wise selection). **Landed on real state**:
   `Editor::extend_word_selection` tracks `WordSelectTouch` (untouched
   / pure backward walk / has had a forward press or retraction), and
   passes the one bit this needs (`retracing`). The remaining peek
   (`retract_onto_the_separator`) only answers "is there a cell to step
   onto," never "should I."
9. **The separator step only accepted whitespace** -- "...arrow-key"
   retracted to "...arrow-k". `MoveWordBackward` always lands at the
   start of a same-class run, so whatever is left of it is never more
   of the same word: the step now takes any existing left neighbor, no
   classification.
10. **A fresh backward selection at the very start of the buffer**
    (nowhere to go) left a phantom one-character selection ("D"). A
    zero-progress check fixed the two-press case but missed another
    route to the same phantom: `Right` then `Left` on the first word
    lands back on the anchor *with* real motion. **Landed on**:
    compare `state.cursor == selection.start` directly and close --
    it catches every route to "nothing left selected."
11. **Retracting past the anchor of a mid-buffer selection** ("Draft
    architecture derived," two `Right`s then `Left`s): the separator
    step ran after landing on the anchor and claimed a space the
    selection had never covered, while the anchor stayed put (" a"
    instead of " "). Fixed by moving the anchor with it, once --
    later superseded by 15.
12. **Fresh backward selection with the cursor on whitespace** (right
    after a word, the normal resting place after typing): the anchor
    kept the space ("derived " instead of "derived"). Extended the
    trim to whitespace anchors.
13. **Mid-word start** ("road|map" selected "roadm"): 7 and 12 were two
    symptoms of one rule. Simplified the trim to "whatever real
    character sits under a fresh backward selection's anchor is never
    included" -- the same convention `shift_select.rs::backward_anchor`
    uses for character-wise `Left`. The append position past a line's
    end has no character, so it's naturally excluded.
14. **The mirror direction had no retracing at all**: `Left`, `Left`,
    then `Right` extended forward from the walk's far end ("t
    architecture"). A forward press on a `NativeBackward` selection now
    retraces (`retreat_forward_through_a_backward_walk`, plain
    `MoveWordForward` -- the exact mirror of the action that built the
    walk) and closes once it reaches or passes the anchor. `>=`, not
    `==`: the trimmed anchor is never on `MoveWordForward`'s own grid,
    so the last press overshoots it.
15. **One word forward, then straight back** left " " selected: 11's
    "claim new territory past the anchor" step fired on the very first
    retraction. Removed it -- landing on the anchor always closes. The
    case 11 was built for still comes out identical, via an ordinary
    fresh backward selection on the next press.
16. **Closing a retraced backward walk landed one column short**
    ("deri" retraced ended between 'r' and 'i'): it snapped to the
    *trimmed* anchor. Now restores `Editor::word_select_true_anchor`,
    captured before any trim. From there a further `Right` starts an
    ordinary fresh forward selection -- matching VS Code once mirrored.
17. **Forward extension stuck on a non-ASCII character** ("... default
    — commit ..." never grew past "default"). `edtui`'s
    `CharacterClass::Unknown` never equals itself, so
    `MoveWordForwardToEndOfWord` breaks before moving at all.
    `advance_over_a_non_ascii_run` walks such a run by hand (mirroring
    `edtui`'s own lead-in: skip empty rows, then ASCII whitespace), and
    the `edtui` action stays the fallback for everything else.
    `MoveWordBackward` has the same comparison but happens to land on
    the character anyway, so it was left alone.

## `WordSelectTouch` details

- **Three states, not `Option<bool>`.** A selection built some other
  way (a whole line selected, then trimmed with `Ctrl+Shift+Left`) needs
  the same full retraction as `Untouched`, while a pure backward walk
  (`NativeBackward`) must keep walking. A boolean collapses the first
  two into one value it then can't tell from the third.
- **`NativeBackward` survives a retracing forward press.** Otherwise the
  second `Right` of a retraction fell through to the ordinary forward
  branch -- sometimes landing right by coincidence, sometimes leaving a
  stray one-character selection on the anchor. A genuine forward press
  past the anchor still becomes `Touched`.
- **Known gap, not chased:** nothing resets the state to `Untouched`
  when a selection closes and a new one is built some other way. Only a
  leftover `NativeBackward` gives a wrong answer (`Touched` retracts
  like `Untouched`), and only if that new selection's first action is
  `Ctrl+Shift+Left` -- too narrow for a bigger hook.

## General lessons

- Before generalizing a fix to "all four directions," check that the
  directions are really the same kind of motion (they weren't for
  character-wise `Shift+arrows` either -- see `shift-select.md`).
- A decision that keeps failing when read from characters usually
  needs real state instead (8).
- Several special cases in a row often turn out to be one rule (13).
