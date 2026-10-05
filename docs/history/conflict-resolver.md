# Conflict resolver (`src/conflict/`) -- history

Code: `src/conflict/`, `src/ui/conflict.rs`. Related: `compare.md`.

## Requested shape

Requested with two Araxis Merge screenshots of a real SVN merge
conflict (`X`, `X.working`, `X.merge-left.rN`, `X.merge-right.rN`):

- **Opened by `Alt+F5` with exactly those four files marked**, the same
  key as Compare; two marked files still open Compare. Chosen over also
  opening it from the cursor on `X` alone.
- **Top, about three fifths of the height: three panes**, as in Araxis
  -- `.working` | `X` with its conflict markers (twice as wide) |
  `.merge-right`.
- **Bottom: `.merge-left` -> `.merge-right`**, an ordinary Compare of
  what the incoming revision changed (the second screenshot), rather
  than `.working` against `.merge-right`.

## Stages

1. Recognizing the four files, the five editors in that layout, `Tab`/
   `Shift+Tab` and the mouse to move between them, `Ctrl+S`, `Esc`.
   `.merge-right` is open twice, as two independent buffers -- it's
   SVN's scratch copy, read rather than edited.
2. The conflicts in `X` are highlighted (`markers.rs`, `ui/conflict.rs`),
   after VS Code's merge colors: marker rows bold on a `warning` wash,
   my section green, the base gray, theirs blue -- blended over `bg` at
   render time rather than new `Theme` fields. `F7`/`F8` did nothing in the top
   panes at first, then jumped from one conflict's `<<<<<<<` to the
   next -- reported as skipping too much. Now each top pane steps
   through its hunks against the result, as in Compare, and the result
   (diffed against both sides) also stops on every marker row
   (`ConflictState::stops`). A hint row with the keys and a count of
   the conflicts left was dropped on request: the height is worth more
   to the editors. A conflict missing its
   `=======` or `>>>>>>>` isn't one, so half-deleted markers stop being
   highlighted.
3. The side panes are diffed against the result every frame and color
   what they have that the result doesn't -- `.working` green,
   `.merge-right` blue. Outside conflicts the result colors each line by
   where it came from: differing only from `.working` means theirs
   (blue), only from `.merge-right` mine (green), from both an edit of
   its own (the marker wash, not bold). The top panes scroll with the
   focused one (`align_top_panes`, `map_real_row` through the result),
   so each keeps its own cursor while unfocused
   (`ConflictState::saved_cursors`), as Compare does.
4. Planned: taking one side of a conflict into `X`.

Only SVN's merge naming is recognized. An `svn update` conflict names
its files `X.mine`, `X.rOLD`, `X.rNEW` instead -- not requested yet.

## Find file opens it too

The first version only checked the panel's marks. The four files were
first tried from Find file's results (a search for the file's name
lists all of them), where `Alt+F5` still knew only "exactly two
marked" and did nothing. Both routes now go through
`conflict::open_resolver`.
