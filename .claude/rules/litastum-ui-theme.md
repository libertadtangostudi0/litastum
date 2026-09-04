# litastum: UI reference (design target for stage 3+)

Written before implementation; `theme.rs` (color values below),
`panel.rs` (column-major layout, file-type coloring), and `ui.rs`
(rendering) now implement most of this — see `ARCHITECTURE.md` for the
module-level plan.

## Color theme ("color 2", GitHub Dark inspired)

- Background `#0d1117`, raised surfaces `#161b22`, borders `#30363d` /
  `#21262d`
- Text primary `#e6edf3`, secondary `#8b949e`, muted `#6e7681`
- Accent (focus / cursor row / links) `#58a6ff`
- Status colors: success `#3fb950`, danger `#f85149`, attention/marked
  `#d29922`

Only F8 (Delete) uses `danger`; every other F-key label uses `accent`.

## Layout direction

Each panel gets its own bordered frame — path on the top edge,
item-count/free-space on the bottom edge — and the file list itself
renders in **2 columns, column-major fill** (fill column 1
top-to-bottom, then column 2), matching Far Manager's classic "Brief"
multi-column view, specifically so arrow keys can navigate efficiently
through long listings (Up/Down within a column, Left/Right across
columns).

No folder icons — directories are still distinguished only by a
trailing `/`, not a glyph. File-type **coloring**, though, reverses an
earlier "deliberately no color dots" decision: entries are now colored
by a coarse category (`panel.rs::HighlightRole` — directory/archive/
executable/other), Far Manager-style, per explicit later request. See
[[litastum-theming]] for the category list and which `Theme` color each
maps to.

## Reference mockup

A static HTML mockup demonstrating this (dark theme, 2-column panels,
arrow-key nav, F-key bar) was reviewed while planning — not committed to
the repo, but its exact colors/behavior are captured above and in
`ARCHITECTURE.md`'s `theme.rs` / column-navigation sections.
