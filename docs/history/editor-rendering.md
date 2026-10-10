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
- **Bracket matching** (`bracket_match/`) highlights both brackets of
  the pair, as in Far/VS Code -- requested directly. `<>` was added on
  request despite being comparison operators too: a depth scan with no
  syntax awareness can mismatch on `a < b`, the tradeoff most editors
  with `<>` matching accept; generics and tags are the common case. It
  stays a separate pass from word highlighting, so neither's rules
  (both brackets vs. "not the one under the cursor") leak into the
  other.
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

## Word-occurrence highlighting (`word_highlight.rs`)

Requested with a screenshot of VS Code's behavior. `TODO/editor.md`
expected a second hand-rolled render pass; reading `edtui`'s
`view/internal.rs` showed `EditorState::highlights` is already
rendered every frame, between syntax styling and the selection -- the
layering this needs. A `Highlight`'s style replaces the span outright
(`InternalSpan::split_spans`), so it sets both `fg` and `bg`, like the
selection does.

- **Cursor right after a word highlighted nothing** -- the append
  position at a line end, or `theme|.rs`. `word_at` checks the cell
  under the cursor, then the one to its left.
- **Crash on a short line.** `MoveUp`/`MoveDown` change only
  `cursor.row`, so moving from a long line onto an empty one left
  `cursor.col` at 35; `row[cursor.col - 1]` panicked. Both branches
  check `< row.len()`.
- **One enormous line** (an escaped log dump with literal `\n`) made the
  editor sluggish: this scan ran on every redraw, O(line length). Lines
  over `MAX_HIGHLIGHTED_LINE_LEN` (20,000, twice VS Code's ~10,000
  tokenization cap) are skipped here and turn off syntax highlighting
  for the file.

## Search jumps land in the middle of the view

Reported with a screenshot: a `Ctrl+F` match further down showed up on
the view's last row -- `edtui` only scrolls just enough to bring the
cursor into view. `Editor::jump_cursor_to` now centers the match;
near the start and end of the file the view stops at the first/last
line. Compare's hunk jumps use the same method (`compare.md`).

The first version had two gaps, reported from Compare ("not always
centered, away from the start and end"):
- **Wrapped lines.** It counted buffer rows, but `edtui` wraps long
  lines -- common in Compare's half-width panes -- so wrapped rows
  above pushed the target below the middle, and `edtui` then scrolled
  again, leaving the other pane a row off. The middle is now measured
  in screen rows (`centered_top_row`: character wrap at the text width,
  the view minus border and line-number gutter).
- **A visible target didn't scroll** (copied from VS Code's search);
  one in the lower half stayed there. Every jump centers now.

## A click right of a line put the caret before its last character

Reported as the mouse caret landing one character left, in every editor
(F4, Compare, the resolver). `edtui` maps a click past a line's text to
its last character -- vim's Normal mode has no cell past it -- and does
so on the release too. First fix: ask `edtui` about the cell one to the
left and move the caret past the end when it landed on the same
character; it still went wrong now and then (reported with an "a"), and
leaned on `edtui`'s guesses. Now the position is ours
(`editor/editor/click.rs`, requested: not tied to `edtui`): the same
layout `edtui` draws -- border, line-number gutter, character wrap at
the text width, a tab two cells, other characters their Unicode width --
read back for the press, the release and a drag's end. `edtui` still
handles the event first (the selection, its mode). Pinned by
`every_drawn_character_is_where_a_click_on_it_lands`, which clicks every
character `edtui` actually drew.
