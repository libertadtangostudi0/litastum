# Character-wise selection (`Shift+arrows`) -- history

Code: `src/editor/bindings/shift_select.rs` (`anchor_fresh_shift_selection`,
`exclude_landing_column_on_fresh_vertical_selection`,
`close_selection_if_back_on_the_anchors_row`), the fresh `i(...)`
`Shift+arrow` entries in `bindings::standard_key_handler`, and
`Editor::vertical_shift_anchor_col`. Called from `Editor::input`.

## Current behavior, in short

- N `Shift+Right`/`Left` presses select exactly N characters.
- `Right`: a fresh selection is the cell the cursor is on (falls back to
  a real move only where there's no character, past a line's end).
- `Left`: a fresh selection is the cell the cursor moves *onto* -- the
  move always happens, then the anchor is dragged along to that one
  cell. No progress (line start) is left to the line-wrap check, so it
  can cross into the previous line.
- `Up`/`Down` keep the plain `SwitchMode(Visual)` + `MoveUp/Down` chain,
  plus a one-time trim so the landing column on the far row isn't
  selected, and a snap back to the untrimmed column
  (`vertical_shift_anchor_col`) when the cursor returns to the anchor's
  row, which also closes the selection.

## The root quirk

`edtui`'s selection is inclusive on both ends (vim-style), and
`SwitchMode(Visual)` alone already anchors a one-character selection on
the current cell (vim's `v`). The fresh `Shift+arrow` table entries used
to chain a `Move` on top of that, so one keypress selected two
characters ("Dr" instead of "D"). First accepted as a side effect of
reusing vim-style actions; fixed once reported against real use -- the
keymap's stated goal is VS Code/Windows convention, where one press is
one character.

## Attempts

1. **Drop the chained `Move` from the fresh entries for all four
   directions, stop on the anchor cell if it's real.** Correct for
   `Right`.
2. **`Shift+Left` selected (and copied) the character to the *right*.**
   The anchor cell is only the right first character for a *forward*
   selection; for a backward one it's always the cell the cursor moves
   onto. Split the directions: `Left` moves first, then drags the anchor
   to the new cell (spanning old-to-new would be two cells again).
3. **`Shift+Down` stopped moving to the next line at all** (selected one
   character right, then landed a column off). `Up`/`Down` never had the
   N+1 bug -- a row jump has no single-character granularity -- and the
   original `SwitchMode(Visual).chain(MoveDown(1))` was already the
   wanted behavior. Scoped the fix to `Left`/`Right` only.
   **Lesson**: `Left`/`Right` and `Up`/`Down` aren't the same kind of
   motion just because both are `Shift+arrow` -- check before
   generalizing a fix to "all four directions."
4. **`Up`/`Down` had a different real bug**: `MoveUp`/`MoveDown` only
   change the row, so the cursor lands on the identical column, and the
   inclusive model grabbed whatever sat there. Reported against aligned
   text (two lines both reading `"xxx.rs    — LATER..."`): one
   `Shift+Down` right before the word swept the destination line's copy
   of it in. Fixed with a one-time trim on the press that opens the
   selection -- `Down` backs the destination (cursor, in lock-step with
   `selection.end`) off one column; `Up` trims `selection.start` instead
   (the row the press started on, now the bottom edge, which `MoveUp`
   never touches). Checked against `edtui`'s multi-line convention
   (whichever raw field sits on the larger row is an inclusive bound on
   that row) after a first attempt nudged `selection.start.col` the
   wrong way and made it worse.
5. **That broke round trips** (reported in a later session):
   `Shift+Down` then `Shift+Up` no longer returned to an empty selection
   -- `MoveUp`/`MoveDown` carry the trimmed column forward forever, so
   the reversing press couldn't land back on the real anchor. A full
   revert of 4 was tried first and immediately reported broken again
   (the aligned word came back). **Landed on keeping both**:
   `Editor::vertical_shift_anchor_col` records the untrimmed column on
   the fresh press (nowhere in `EditorState` could hold it -- `Up`'s own
   trim already mutates `selection.start`), and
   `close_selection_if_back_on_the_anchors_row` restores it and closes
   the selection whenever the cursor returns to the anchor's row --
   `edtui` can't represent an empty selection any other way.
6. **`Shift+Down` from column 0 took the next row's first character**
   -- the trim in 4 has no column before 0 to back off to, so it left
   column 0 alone. A paste or cut over the selection then ate that
   character. The end now sits on the line break of the row above
   instead (`(row, len)`, which `edtui` counts as the break -- checked
   with its own `DeleteSelection`), with `VerticalAnchor::ends_on_line_break`
   set so further presses move from break to break
   (`follow_line_break_selection`); `Shift+Up` from column 0 moves the
   bottom bound to the break above the anchor the same way. The anchor's
   row is now stored too (`VerticalAnchor::position`), since that bound
   is no longer on it. The caret shows at the end of the last selected
   line rather than the start of the next -- `edtui` places it from the
   cursor, which has to stay on the selection's end.
7. **`Shift+Home`/`Shift+End` dropped the selection** -- only the plain
   keys were bound. Now bound; a fresh `Shift+Home` moves the anchor off
   the cell under the caret, and `Shift+End` stops on the last character
   rather than the line break `MoveToEndOfLine` lands on in `Visual`.
8. **Deleting a fully selected line removed it, break and all** --
   `edtui`'s `DeleteSelection` behaves like vim there, so a paste over a
   line selected with `Shift+End` ran into the next one. `Backspace`,
   `Delete`, `Ctrl+X`, typing and paste over a selection now go through
   `Editor::delete_selection` (`selection_delete.rs`), which only takes
   a line break that's actually selected.

The correction passes now live in one function,
`bindings::correct_after_dispatch`, which both `Editor::input` and the
binding tests' harness call -- they had been two copies.
