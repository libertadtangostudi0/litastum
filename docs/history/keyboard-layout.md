# `Ctrl` shortcuts under a non-Latin layout -- history

`Ctrl+C` in the editor did nothing under a Russian layout. The log
showed `Char` U+0441 (Cyrillic) with `CONTROL`, not Latin `c`:
the Windows console (`ReadConsoleInputW`) translates the key through the
active layout even with `Ctrl` held. Every `Ctrl+<letter>` binding in
the app was affected, not just the editor's, and any non-Latin layout
would do the same. Unix terminals send control bytes, so they never had
the problem.

`normalize_ctrl_shortcut` maps the letter back through per-layout
position tables. There's deliberately no "active layout" state:
`crossterm` doesn't report it, and searching every table stays correct
across a layout switch.
