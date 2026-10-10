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
- **Editable path titles.** Reported with a screenshot: a long path in a
  pane's top border showed only its start, cutting off the file name, and
  couldn't be changed. The title now keeps the path's end behind a `…`
  (`view.rs::fitted_title`, every editor), and a click on it or `Ctrl+L`
  turns it into a field (`compare/path_edit.rs`, since moved to `src/path_edit.rs` and shared with F4's save-as and the panels) -- `Enter` loads the
  typed path into that pane, as in Araxis; the diff follows on the next
  frame. A pane with unsaved changes isn't replaced. The conflict
  resolver's panes work the same way. The field first drew `theme.text` on the
  selection color, unreadable on a scheme with a bright one; filling the
  whole field with the panels' selected-row style (`selection_text`)
  then made a selection look like the fill being removed. Now the text
  is plain and only the selection gets that style.
- **Moving the caret crawled on large files, worse at a small font**
  (reported from the window with the font zoomed out): every frame built
  both texts and diffed them (~1.3 s a frame for two 20k-line files in a
  debug build), then handed `edtui` a highlight for every changed row of
  the file, which it checks per visible line -- so the more lines on
  screen, the slower. The diff is now kept until a text changes
  (`DiffCache`, keyed on `Editor::revision`), highlights are rebuilt only
  with it, and a frame passes `edtui` just the visible rows' highlights
  (`highlights_on`). A frame with a caret move: 1.3 s -> 2 ms (160x45),
  -> 18 ms (400x120). The conflict resolver caches its two diffs and its
  conflicts the same way.
- **Selecting by word did nothing in Compare** (reported), nor did
  `Ctrl+A`, and `Esc` with a selection closed Compare: keys went straight
  to `Editor::input`, past F4's own layer, where word selection and
  select-all live. F4, Compare and the conflict resolver now share it
  (`editor_keymap::text_key`), and `Esc` cancels a selection first
  (`cancel_selection`). The rest of selection editing (`Shift`+arrows,
  `Shift+Home/End`, typing or `Backspace` over a selection, cut/paste,
  mouse) already went through the same `Editor`; tests pin it now.
- **Only the change is highlighted** (reported with Araxis screenshots:
  commenting a line out lit the whole line). A changed line with a
  counterpart on the other side -- the same row of a replaced block --
  gets a faint background, and only the characters that differ the
  diff's color (`diff::inline_changes`: a character diff, changes with
  at most 2 unchanged characters between them merged, whole for lines
  over 2000 characters). Lines added or removed as a whole stay
  strong. The conflict resolver's side panes do the same against the
  result. The character highlights come before the line's: with no
  syntax colors, `edtui` lets the first of two overlapping ones win.
- **No hint rows** (requested: the screen should be the editor's): F4
  and Compare drop their key-hint rows, as the conflict resolver already
  had. F4's `[modified]` lived in that row; it's in the border title now,
  after the path, for every editor pane (`Editor::title`).
- **The wheel sometimes did nothing** (reported with a touchpad, "and
  it lags"). Measured: a frame was ~1.5 ms, not the cause. `edtui`
  scrolls only the view on the wheel, then pulls it back on the next
  frame to keep the caret a few rows off the edge -- so scrolling
  stopped dead once the caret got there (on the project's own sources,
  every wheel frame changed nothing). `Editor::scroll_lines` now moves
  the caret along, on its screen row, three lines a step (Windows'
  default per notch; litastum's window turns three lines of touchpad
  travel into one step). In the resolver, the wheel over an unfocused
  top pane was undone by the next frame's alignment; it scrolls the
  focused pane now.

