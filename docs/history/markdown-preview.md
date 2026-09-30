# Markdown preview (F3 on a `.md` file) -- history

Code: `src/explorer/markdown_preview/` (`state.rs`, `links.rs`,
`input.rs`), `src/ui/markdown_preview.rs`, the sync in `ui::draw`.

## Editor and preview side by side

`F3` opens the file in the editor (left) with a live rendered preview
(right), refreshed on every `Ctrl+S`. The preview is its own simple
code path -- headings, bold, lists, quotes and code drawn structurally --
and shares nothing with the editor except `reload()` after a save, which
keeps the scroll position (clamped) and leaves the preview untouched on
a read error instead of blanking it.

## Following the editor's cursor

Requested twice. First: scroll the preview with the editor and
highlight the matching line. Then, once that top-aligned version was in
use: keep the two *level* -- editing halfway down the editor's page
should show the matching preview line halfway down too, not at the top.
`ui::draw` computes the cursor's relative position on the editor's page
(heights derived the way `draw_editor`/`draw_preview_frame` compute
their content areas) and `sync_to_editor_cursor` places the line at the
same fraction. Only while the editor has focus, so scrolling the preview
by hand (`Tab` to it) isn't undone on the next frame. Blank separator
lines are skipped when matching: they carry the row their block closed
on, often the same as that block's last real line.

## Links

- **`Ctrl`+click hit the wrong link** after a wrapped paragraph: the
  click's row was mapped back to a logical line (`scroll + row`), which
  drifts once anything above wraps. The renderer now records exact
  per-row link hitboxes from the same wrapped rows it draws, so
  hit-testing can't disagree with the screen.
- **A click gave no feedback** (no link, unsupported anchor, failed
  open, or just "did it register?"). The outcome now shows as the
  panel's bottom border title.
- **That message used to show the raw URL.** Truncated by the border,
  it still looked like a complete URL, and Windows Terminal linkifies
  URL-shaped text on its own -- `Ctrl`+clicking it (a click the app never
  sees) opened a broken address and 404'd. The message is built from
  the link's label instead.
- **`l` opens a link search** -- a filterable keyboard list of every
  link -- as the exact alternative to mouse hit-testing.
