# Editor rendering (`view.rs`) -- history

Code: `src/editor/editor/view.rs`. Related: `word-select.md` (5, 6),
`editor-performance.md`.

## The cursor cell is repainted last

`edtui`'s `EditorView::render` paints the cursor's cell *after*
selection and highlights, unconditionally; `.hide_cursor()` only makes
that overwrite `base` instead of skipping it. So every highlight the
cursor can sit inside needed its own exception in `cursor_cell_style`,
each reported separately:

1. **Selection.** The keymap keeps `state.cursor` on the selection's
   live end, so that cell is the last selected character. Painted
   `base`, pasted text looked one character longer than the highlight
   ("planning chat " pasted as "planning chat +").
2. **Bracket matching.** Once both brackets of a pair were highlighted,
   the one under the cursor was still overwritten -- only the far one
   ever looked highlighted.
3. **`Ctrl+F` search.** A jump puts the cursor on the match's first
   character, which then never looked highlighted. Checked by caret
   position, not by "is the box focused": the box can stay open with the
   caret elsewhere, where it must render normally.
4. **Compare's diff rows (`extra_highlights`).** The cursor cell was a
   plain patch in a red/green row. Reuses the specific highlight the
   cursor is in -- a row can be either color.

Also reported: the character *after* the cursor got a different fill.
That's this same mechanism, and the fix is `None` (hide `edtui`'s block
cursor) wherever none of the cases apply; the terminal's own bar cursor
is what shows.

## The terminal bar cursor during a selection

A bar cursor is drawn at the *left* edge of its cell. On the last
selected character it read as "the selection stops one early", though
render and copy were right (confirmed from logs). `cursor_screen_position`
shifts it one column right -- but only on the selection's trailing edge:
when extending backward, the shift put the bar after the first selected
character ("loaded" retracted onto 'l' showed the bar between 'l' and
'o').

## Highlights

- **One style for word occurrences and bracket pairs**: brackets first
  shipped with their own look and were asked to read as the same kind
  of "matches something nearby" hint.
- **The far bracket of a multi-line pair** only highlighted while its
  row was already visible -- `edtui` only scrolls to keep the cursor's
  row in view. `widen_viewport_to_show_matched_bracket_pair` moves the
  viewport when the pair fits; `edtui` re-adjusts if the cursor would
  leave it, so wrong math can't hide the cursor.
- **While the search box is open, its match is the only highlight.**
  Our `SearchSession` draws it as a plain `Highlight`, and `edtui`'s two
  render paths disagree on which of two overlapping highlights wins
  (first in the plain path, last in the syntax path).
- **Word occurrences only scan rows that can be on screen**
  (`rows_that_can_be_visible`): the whole-buffer scan was ~30ms per
  frame on 100k lines (`editor-performance.md`).
