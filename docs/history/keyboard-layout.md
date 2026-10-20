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

## Letter keys that are commands

The same held for a plain letter that is a command, not text: under
the Russian layout the delete prompt's `y` arrived as `н` and took no
answer (reported). `latin_key` reads such a key by its position too
(`normalize_ctrl_shortcut` is it for `Ctrl` chords): `Y`/`N` in every
prompt (`yes_no::answer`), `Y`/`T`/`N` in the link question, `I`/`E`
in the theme picker, `l` in the Markdown preview, and `Shift+A` (mark
all) on an empty command line -- the browser's table tries a letter no
row takes again by its key (`bindings::lookup`). Never typed text --
the command line, the editor and text fields keep the character typed.
