# Copy / Move (`F5`/`F6`) — landed, gaps left

Far Manager-style: `F5`/`F6` open `Mode::ConfirmTransfer` for the entry
under the cursor, pre-filled with the *other* panel's directory (plus
the entry's own name) as an editable destination — `Enter` runs it
(`fs_ops::copy_entry`/`move_entry`), `Esc` cancels. A directory copies/
moves recursively; a move tries `fs::rename` first and only falls back
to copy-then-delete if that fails (e.g. across drives on Windows, which
always errors rather than transparently copying).

- [x] Copy/move a single entry (file or directory tree) between panels,
      with an editable destination path — full cursor movement
      (`text_field.rs`: `Left`/`Right`, `Ctrl+Left`/`Ctrl+Right` by
      word, `Home`/`End`, `Backspace`/`Delete`), not the command line's
      own append/backspace-only editing — the popup is modal, so
      arrows aren't needed for panel navigation the way they are on the
      always-live command line, freeing them up for real text-cursor
      movement
- [x] `Shift+F6` — rename in place, Far Manager's own binding: opens
      the same `Mode::ConfirmTransfer` prompt as plain `F6`, just
      defaulting the destination to the entry's own directory instead
      of the other panel's, with the cursor starting right at the
      filename (not the end of the whole path) so typing immediately
      edits the name. Needed its own dispatch path in
      `main.rs::handle_browsing_key` ahead of `keymap::resolve`, since
      that table only keys off `KeyCode` (F6), not the Shift modifier
      that distinguishes it from plain move
- [x] `Shift+Left`/`Shift+Right` select a range in the destination
      field (`text_field.rs`'s `selection_anchor`, highlighted with
      `theme.current_row_bg` in `ui.rs::destination_line`) — typing a
      character replaces the selection, `Backspace`/`Delete` remove it,
      plain `Left`/`Right` collapse to the selection's near edge
      instead of moving one further character, all standard text-field
      behavior
- [x] Multi-select — `F5`/`F6` act on every entry currently marked in
      the active panel (`panel/marks.rs`) if any are, falling back to
      the cursor entry otherwise, Far Manager-style. Marking itself:
      `Shift+A` selects every entry except `..`, `Shift+Up`/`Down`
      toggles the cursor row and moves, `Shift+Left`/`Right` does the
      same for the whole column(s) the existing paginated jump crosses.
      A single source still defaults the destination to a full target
      path (rename-during-transfer still works); several default to
      just the target directory, each source's own name joined onto it
      individually. `Shift+F6` rename stays single-entry only — no
      sensible way to rename several files through one free-text field.
      Originally bound to `Ctrl` instead of `Shift` throughout, switched
      after a direct correction — see `panel/marks.rs`'s own doc comment
      for the full binding history, including why `Shift+A` alone stays
      gated to an empty command line (it types a literal capital `A`
      otherwise)
- [ ] No overwrite confirmation — an existing file/directory at the
      destination is silently replaced (`fs::copy`/`fs::rename`'s own
      behavior), unlike Far Manager's own "already exists, overwrite?"
      prompt
- [ ] No progress indicator for a large copy/move — the prompt just
      sits there (frozen, no visible progress) until the whole
      operation finishes; fine for small files, not for a big tree
- [ ] A failed transfer is only logged (`debug!`), not shown to the
      user — same status-bar gap as delete
- [ ] **`copy_dir_recursive` doesn't handle symlinks specially** —
      found during a perf/weak-spot audit pass, not yet reported by
      anyone hitting it in practice. `DirEntry::file_type()` doesn't
      follow a symlink (it's `lstat`-based), so a symlink pointing at a
      directory reads as "not a directory" and falls through to the
      plain-file branch (`fs::copy`), which then fails outright since
      its source path is actually a directory. A symlink pointing at a
      *file* does "work" today, but only by accident: `fs::copy`
      follows it and copies the target's real content, silently turning
      the copy into an ordinary file and losing the symlink itself --
      not obviously the wanted behavior either. Needs a product
      decision before fixing, not just a mechanical patch: should a
      copy *recreate the symlink itself* at the destination (matching
      what most real file managers/`cp -a` do), or deliberately
      *dereference* it (copy whatever it currently points to, as a
      normal file/tree)? Low real-world hit rate on Windows specifically
      (creating a symlink needs Developer Mode or admin rights), higher
      on Unix or for a junction/symlink created by other tooling (WSL,
      git, an IDE). Whichever direction is picked, add regression
      coverage for it in `fs_ops.rs`'s own test module (currently has
      no symlink case at all).
