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
2. **A save notices the file changed on disk**: `mtime` and size kept
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
