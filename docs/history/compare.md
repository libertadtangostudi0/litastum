# File compare (`Alt+F5`) -- history

Code: `src/compare/` (`diff.rs`, `state.rs`), `src/ui/compare.rs`.
Design: `TODO/file-compare.md`.

## Read-only phase 1 -> editable panes

The first version was read-only and inserted synthetic filler rows into
the text to align the two sides, with `[CRLF]`/`[LF]` markers baked
into the text too. The panes became two ordinary `Editor`s (cursor,
undo, highlighting, save), which ruled both out: extra characters in a
real, saveable buffer would corrupt the file on save.

- The diff is recomputed every frame from both panes' live text, so it
  can't drift from what's being edited. Its `Empty` padding rows only
  classify rows and drive `map_real_row`; they're never drawn as text.
- Line-ending markers are an overlay drawn over the editor view, from a
  snapshot taken on open (`edtui` drops `\r` on load, so the live
  buffer can't tell).
- Only the focused pane scrolls on its own; the other's viewport is set
  every frame (`Editor::set_viewport_top_row`). That technique came
  from a real scroll bug in the read-only version: `edtui` re-derives
  the viewport from the cursor on every render.
- Changed rows are one flat color pair: an `edtui` `Highlight`
  replaces the span's style, so the diff background can't sit under
  syntax colors (syntax highlighting is off in Compare for that
  reason).

## Reports

- **`F8` stopped on every line of a multi-line hunk.** It searched for
  the next changed row from `cursor.row + 1`, which is still inside the
  same hunk. `next_hunk_start` now skips the current hunk first.
- **Aligning the other pane** looks *forward* for the nearest row with
  real content, so scrolling to the top of an added/removed block lines
  the other side up with the start of that block's gap, as GitHub/VS
  Code do.
- **`F7`/`F8` for next/previous difference**, requested to match
  TortoiseMerge/`merge.exe`, next to `Ctrl+Up`/`Ctrl+Down` (`Tab`
  already switches panes). The backward jump got the same fix as
  forward: land on the previous hunk's first row, not its last.
- **Syntax highlighting off in Compare**, requested: token colors
  competed with the diff colors.
- **Line endings drift after editing**: the `CRLF`/`LF` snapshot is by
  row index, so inserting lines shifts it. Accepted -- the use case is
  checking a lightly edited file.
- **Hunk jumps land in the middle of the view**, like `Ctrl+F` matches
  (requested right after the search change): `F7`/`F8` move the cursor
  through `Editor::jump_cursor_to`. It first missed wrapped lines and
  skipped targets already on screen (`editor-rendering.md`).
- **The cursor landed below the hunk**, further down the file the
  more lines the other side had inserted (reported with screenshots:
  the left pane 2-3 rows below the red block, the right one better).
  Hunks are found in diff rows, which include `Empty` padding, and that
  diff row was used as the cursor's real row -- and the cursor's real
  row as the search start. `jump_to_hunk` converts both ways
  (`diff_row_of`, `source_index`).
- **Clicking did nothing in Compare**: mouse capture was only on for
  the F4 editor. Now a click in the other pane focuses it (as `Tab`)
  and places the caret there; clicks and drags work like in the editor,
  and the wheel scrolls the focused pane (`CompareState::mouse`).
