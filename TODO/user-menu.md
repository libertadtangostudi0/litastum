# User menu (F2)

- [x] `F2` opens a per-directory user menu (`explorer::user_menu`),
      reading `LitastumMenu.ini` from the active panel's own directory
      -- or, if that's not there yet, a compatible `FarMenu.ini`, which
      gets copied into `LitastumMenu.ini` on the spot (the original
      `FarMenu.ini` is never modified). If neither file exists, creates
      an empty `LitastumMenu.ini` and opens it in the built-in editor
      right away instead of browsing an empty popup -- a first version
      made `F2` a silent no-op here instead, reported confusing
      (indistinguishable from `F2` simply not being bound); an
      empty-popup-with-a-hint version was tried next and rejected too
      (there's nothing to browse yet, so jumping straight to writing
      the file is more direct than a hint pointing at the same thing).
- [x] Real Far Manager's own nested-block grammar (confirmed against a
      published example, `pkjq/far-git-menu`'s `FarMenu.ini`, not
      guessed): `hotkey: title` headers, `{ ... }` submenus, one or more
      command lines per leaf item, `;` comments. `.ini` was picked as
      litastum's own extension specifically *because* it's the same
      grammar either way -- one parser, no format conversion needed for
      the migration copy.
- [x] `!&` (the entry under the cursor) and `!?Label?Default!`
      (interactive prompt, substituted before running -- one popup per
      unique label, `Mode::UserMenuPrompt`) both work, matching real
      Far Manager's own macros.
- [x] Running an item's commands reuses the command line's own
      `command_line::run_shell_command_lines` (suspends the TUI,
      inherits stdio, pauses for a keypress) -- extracted from
      `run_command_line` specifically so both share one implementation.

## Known gaps

- [ ] Hotkey letters (`s:`, `l:`, ...) are parsed and shown as a prefix
      in the list, but aren't wired up as instant-select shortcuts --
      `Up`/`Down`/`Enter` only, matching this codebase's own existing
      precedent for the F9 menu (`TODO/f9-menu.md`'s "no keyboard
      shortcut letters" gap). `F1`..`F24`-style hotkeys (real Far
      allows those too) aren't recognized as hotkeys at all --
      `parse::parse_header`'s own doc comment has the detail.
- [ ] No "danger" confirmation before running an item -- real Far
      doesn't have one either for a plain user menu, so this matches,
      but worth remembering if a future item type needs one.
- [ ] Long item lists aren't scrolled, just clamped to the terminal
      height, same gap `TODO/f9-menu.md` already lists for the F9 menu
      and color-scheme picker.
- [ ] No global (config-dir) or user-profile-level menu, only the
      per-directory one real Far Manager also supports -- not asked
      for, not attempted.
