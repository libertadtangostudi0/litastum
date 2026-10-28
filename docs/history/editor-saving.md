# Saving files -- history

Code: `src/editor/editor/file_write.rs` (`write_in_place`), used by
`Editor::save`/`save_as` -- F4, Compare and the conflict resolver alike.

## The choice

Requested: saving made safe. An ordinary file is written over in place
-- on Windows, keeping hidden attributes and alternate data streams --
and flushed to the disk. Writing a temp file and renaming it over isn't
used for it. What makes it safe besides is two more things: a save
notices the file changed on disk since it was read and offers to
compare or overwrite, and unsaved edits are backed up a moment after
they're made, restored after a crash.

Weighed and turned down: a temp file next to it and `ReplaceFileW`.
More atomic, and Windows carries attributes and permissions over, but
hard links break, some streams go, and a crash leaves the temp file.

The plan, in stages:
1. **Write over the file and flush it** -- done, below.
2. **A save notices the file changed on disk** (overwrite / cancel done, below; compare next): `mtime` and size kept
   at open and at each save; changed, the save asks -- overwrite /
   compare (`Alt+F5`, our Compare) / cancel.
3. **Backups of unsaved edits**: a copy of the text in
   `appdata/backups/` a second after an edit; offered back when the
   file is opened next; gone on save or on closing unchanged.

## Stage 1: written over, flushed

`fs::write` already wrote over the file (Rust opens an existing file and
cuts it, so a hidden file and its streams stayed -- checked). But it cut
the file to nothing first, so a failure part-way left it empty, and it
didn't flush, so after a power cut the save could be lost.
`write_in_place` writes the new text from the start, cuts the old tail
after it (a failure leaves the start written, never an empty file) and
`sync_all`s before the save counts as done. Pinned: a hard link keeps
seeing the file (a rename would cut it), a hidden file stays hidden
with its other streams.

## Stage 2: a save notices the file changed on disk

Each editor keeps its file's modification time and length as read and
as last saved (`file_write::DiskStamp`). `Editor::save_checked` writes
only if the file is still so; changed -- or gone -- it writes nothing,
and `editor::save_focused_or_ask` asks (`Overlay::ConfirmOverwrite`,
"Changed on disk"): `Y` writes over it, `N`/`Esc` go back to editing.
One path for F4, Compare's and the resolver's panes (their own
`save_focused` went). The length is compared too: two changes within
the file system's time resolution would look the same by time alone.
`Shift+F2` (save as) asks about an existing target already.


## Line breaks and indentation kept

Reported: after a save in the editor, VS Code showed the file's
indentation broken. Two causes, found by a probe saving real files:

- **The save wrote `\n` throughout.** `edtui` drops the `\r` of each
  `\r\n` when it loads a text, and the save joined the rows with `\n`,
  so a CRLF file came back LF. The file's line breaks are now read at
  open (`line_endings.rs`) and written back; an untouched CRLF or LF file
  saves byte for byte as it was (BOM, no last line break included).
- **Tab typed `\t` into a file indented with spaces.** The file's
  indentation is now told at open (`text_format::Indent`, from the
  indented lines: tabs or spaces, and the most common step between
  indents as the width) and Tab types it: spaces to the next stop, or a
  tab (`indent.rs`). Tab over a selection that takes a line break
  indents its lines; Shift+Tab takes a level off the line or the
  selected lines. It was unhandled: `edtui` panics on `BackTab`, so it
  was swallowed.

A file with both line breaks says `Mixed` on the bottom border. F9 ->
Line endings picks LF or CRLF for the next save, which then writes it
throughout -- a change of the file, so the editor shows `[modified]`
(picking the file's own again undoes that). Until line breaks are kept
per line, an edited mixed file is saved with the one most of its lines
use. Planned next: per-line line breaks, so a mixed file changes only
where it was edited; Tab following the indentation of the lines around
the caret rather than the whole file's; encodings.

## Enter keeps the indentation, Backspace takes a level

Requested next, as the main source of uneven indentation left: Enter
(`edtui`'s) started every new line at column 0, so each line's
indentation was typed by hand, and Backspace took one space of a
four-space Tab at a time -- three presses short left a line off the
grid. `auto_indent.rs`:

- Enter copies the line's own indentation, character for character (so
  a mixed file isn't made more mixed), a level more after `{`, `(`, `[`
  -- and `:` in Python and YAML only (a Rust label isn't a block).
  Between a bracket and its closer the closer goes a line further down.
  The spaces at the split are dropped; Enter at or before the
  indentation moves the line down whole, leaving no spaces behind.
- Backspace with only spaces before the caret goes back to the previous
  stop; after text, after a tab or for a single space it deletes one
  character as before.
- A paste typed in as keys (Windows Terminal's right-click paste) would
  have been indented again line by line: an `Enter` or `Tab` queued in a
  burst of typing is taken as written (`event_loop::keys::burst_text`).

Vim keeps `edtui`'s own Enter and Backspace.
