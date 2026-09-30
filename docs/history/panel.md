# File panel layout and navigation -- history

Code: `src/explorer/panel/mod.rs` (`Panel::column_height`,
`ensure_selected_visible*`, `move_left`/`move_right`), rendering in
`ui::draw_entry_grid`.

## Column-major scrolling

1. **No scrolling at all.** A directory with more entries than fit the
   panel's height just ran off the bottom: the renderer handed each
   column's entries to a plain `ratatui::widgets::List` with no
   `ListState`, which doesn't auto-scroll.
2. **Column 2 started at the wrong entry** once scrolling existed. The
   column height was `visible_rows` unconditionally -- right once the
   list is longer than a page, wrong for a short list that fits: column
   1 then started at `visible_rows` instead of the list's own even
   split, leaving column 0 with a gap and later columns empty (reported
   against a real 101-entry directory). `column_height()` is now
   `min(visible_rows, rows())`.
3. **`Left`/`Right` scrolled by one row** at first, like `Up`/`Down`,
   and felt wrong: a column jump moves `column_height` entries, so a
   one-row nudge left the cursor at an arbitrary row. They page by a
   whole column instead (`ensure_selected_visible_paginated`).
4. **`Right` from the last column did nothing** while a partial page
   remained (reported against a 102-entry directory, the trailing
   `generate_test_files.bat` unreachable). From the last column it now
   jumps to the very last entry. In any other column a missing row
   still means "stay put" (an unevenly split short list). `Left` needs
   no mirror case: `saturating_sub` already floors at entry 0.

## Natural sort

Plain `to_lowercase().cmp()` put `100.txt` between `10.txt` and
`11.txt` (reported against a real 100-file directory). Names now sort
naturally -- digit runs by numeric value -- like Far's own panel.

## Marks keyed by name

Marks are a set of names, not indices, so they survive an in-place
`reload()` (a file changing on disk) while the entry still exists; an
index set would silently point at the wrong entry once sorting or the
count shifted. They're cleared on an actual directory change instead.

## Marking

Marks were first bound to `Ctrl`+arrows and moved to `Shift` after a
correction. `Shift+Left`/`Right` were asked to "mark the whole
column(s) it jumps over", the column-sized version of `Shift+Up`/`Down`.
