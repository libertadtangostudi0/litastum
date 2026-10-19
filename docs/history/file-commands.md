# Opening entries from the browser (`explorer/command/open.rs`) -- history

- **`Enter` on a file was a no-op**; only `F4` opened it. Requested:
  it opens the built-in editor, and so does `Shift+Enter`. The two only
  differ on a directory (navigate into it vs. hand it to the OS file
  manager).
- **`Shift+Enter` on `..` opened the parent's parent.** The first
  version resolved `..` to `panel.path.parent()`, like
  `Panel::enter_selected`. Reported on retest: `..` should open the
  directory being browsed (`panel.path`) -- it isn't an entry with
  somewhere else of its own to show.
- **`F2` with no menu anywhere was a silent no-op**, indistinguishable
  from an unbound key. It now creates an empty `LitastumMenu.toml` and
  opens it in the editor. That file was first created in the active
  directory, which scattered commented-out templates into every
  directory `F2` was pressed in; after the common-menu fallback landed,
  it's created only in the common config directory
  (`user_menu::common_menu_dir`).
- **`FarMenu.ini` is offered, not read or converted silently**
  (`Overlay::ConfirmPortFarMenu`), and wins over an existing
  `LitastumMenu.toml`.
- **`Shift+F6` renames in place**: the same transfer prompt as a move,
  defaulting to the entry's own directory, with the cursor at the name.

## F8 to the Recycle Bin, Shift+Del for good

Requested, as safety: `F8` deleted for good (`remove_dir_all`). It
sends to the Recycle Bin now, through the shell's own file operation
(`SHFileOperationW`, `FOF_ALLOWUNDO`), as Explorer does; `Shift+Del` on
an empty command line deletes for good, as in Far. The prompt says
which ("To the Recycle Bin" / "Delete for good"). Outside Windows there's
no Recycle Bin and the prompt says it's for good. Tests delete for good
too: they must never fill the Recycle Bin of whoever runs them, so the
shell call itself isn't under test.

