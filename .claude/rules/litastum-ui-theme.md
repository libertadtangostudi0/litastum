# litastum: UI reference (design target for stage 3+)

Not yet implemented in code as of this writing — see `ARCHITECTURE.md`
for the module-level plan (`theme.rs`, column-major `Panel` layout) that
implements what's described here.

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

No folder icons, no file-type color dots — directories are distinguished
only by a trailing `/`. This is a deliberate minimalism choice, not an
oversight.

## Reference mockup

A static HTML mockup demonstrating this (dark theme, 2-column panels,
arrow-key nav, F-key bar) was reviewed while planning — not committed to
the repo, but its exact colors/behavior are captured above and in
`ARCHITECTURE.md`'s `theme.rs` / column-navigation sections.
