# User menu (`F2`) -- history

Code: `src/explorer/user_menu/` -- `parse/` (the `FarMenu.ini` DSL,
macros), `state/` (files, porting, browsing), `input/`. Far's macros:
its `@MetaSymbols` help topic.

## Macros

- **Two macro families.** Far's own `!...!` is supported for ported
  `FarMenu.ini` files and habit, but it collides with cmd.exe's
  delayed-expansion `!VAR!`. litastum's own `{{...}}` collides with
  nothing in cmd, PowerShell or POSIX `sh`. Only the two requested
  exist so far: `{{cursor}}` (Far's `!.!`) and `{{prompt:...}}` (Far's
  `!?...?!`).
- **Prompt placeholders stopped opening the prompt** and ran the raw
  command instead. Substitution copied only the opening `!?` and
  rescanned the rest character by character, so the placeholder's
  closing `!` was read as a new bare-`!` macro and eaten;
  `extract_prompts` then found no closing `!`. `!?...?!` and `!?!` are
  now copied whole.
- **Deliberate gaps against Far**: 8.3 short names fall back to the long
  name (no short-name lookup, no such thing off Windows); `!=\`/`!=/`
  (symlink-resolved path) fall back to the plain path; `!?!` (Far's
  `descript.ion` descriptions) stays literal; `!@!`/`!$!` (a list
  *file*) inline the list like `!&`, keeping substitution I/O-free.

## Browsing and editing

- **Hotkeys were shown but did nothing**: parsed and displayed as a
  row prefix, never bound. Now a letter selects and runs/enters the
  item at the current level.
- **`Right` ran commands.** `Right`/`Left` next to `Enter`/`Esc` were
  requested (Far's menus accept both), but an earlier version made
  `Right` identical to `Enter`, so arrow browsing could fire a real
  command. `Right` now opens a `Commands` item for editing.
- **Edits deep in a submenu were lost.** Submenus were cloned into
  their own levels on entry -- fine read-only, wrong once editing came:
  an edit three levels down changed a clone that `back()` dropped.
  Every access now walks `root` through an index `stack`.
- **`F4` on an item, third try.** The first version opened the whole
  `LitastumMenu.toml`, the second a one-line in-popup field
  (`EditUserMenuItemState`); both missed the ask -- just this item's
  commands, in the real editor. Now a scratch file with one command per
  line, read back when the editor closes (saved or discarded alike).

## Files and porting

- **`FarMenu.ini` ported to zero items.** A real Far export is UTF-16LE
  with a BOM (`FF FE`); `fs::read_to_string` rejected it. Encoding is
  detected by BOM now.
- **A `FarMenu.ini` next to an existing menu was ignored**, so there
  was no way to re-import one short of deleting `LitastumMenu.toml`.
  It now takes priority and is offered for porting. Porting backs up the
  existing TOML to `.bak`, and `FarMenu.ini` moves to `FarMenu.ini.bak`
  either way (ported or declined) -- the very first version left it in
  place, which would now re-prompt forever.
- **The menu vanished in another directory** (a Subversion working copy
  far from where it was set up): only the active directory was checked.
  A common menu in the config directory is the fallback now, Far-style.
- **An empty local template shadowed the common menu**: directories
  where `F2` had been pressed before the fallback existed held a
  commented-out template, found first forever. A local file with no
  items no longer counts.
- **Creating the menu in the config directory failed** when that
  directory didn't exist yet (nothing ever saved there); it's created
  first now.
