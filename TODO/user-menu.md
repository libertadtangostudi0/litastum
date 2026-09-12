# User menu (F2)

- [x] `F2` opens a per-directory user menu (`explorer::user_menu`),
      reading litastum's own `LitastumMenu.toml` from the active
      panel's directory. If neither it nor a `FarMenu.ini` exists,
      creates an empty `LitastumMenu.toml` (with a commented-out
      example) and opens it in the built-in editor right away instead
      of browsing an empty popup -- a first version made `F2` a silent
      no-op here, reported confusing (indistinguishable from `F2`
      simply not being bound); an empty-popup-with-a-hint version was
      tried next and rejected too (there's nothing to browse yet, so
      jumping straight to writing the file is more direct).
- [x] **Format switched from a `FarMenu.ini`-compatible `.ini` to a
      native `LitastumMenu.toml`** (serde-backed, `toml_format.rs`),
      once it became clear the UI would eventually need to
      add/remove items programmatically -- a structured format with
      real serde support is a plain `Serialize`/`Deserialize` round
      trip for that, where the original hand-rolled DSL would have
      needed its own serializer to write back. `parse.rs`'s DSL parser
      (confirmed against a published example, `pkjq/far-git-menu`'s
      `FarMenu.ini`, not guessed: `hotkey: title` headers, `{ ... }`
      submenus, command lines, `;` comments) is kept, but now only as a
      one-time *read* path for porting a `FarMenu.ini`, never for
      litastum's own file.
- [x] A `FarMenu.ini` found with no `LitastumMenu.toml` yet is
      *offered*, not read or converted silently
      (`Mode::ConfirmPortFarMenu`, `Y`/`N`) -- `state::port_far_menu`
      does the actual DSL-parse-then-TOML-serialize once confirmed;
      `FarMenu.ini` itself is never modified either way.
- [x] `!&` (the entry under the cursor) and `!?Label?Default!`
      (interactive prompt, substituted before running -- one popup per
      unique label, `Mode::UserMenuPrompt`) both work, matching real
      Far Manager's own macros -- these live in `parse.rs` too, since
      they're substituted into a `Commands` item's strings regardless
      of which file format produced them.
- [x] Running an item's commands reuses the command line's own
      `command_line::run_shell_command_lines` (suspends the TUI,
      inherits stdio, pauses for a keypress) -- extracted from
      `run_command_line` specifically so both share one implementation.

## Next

- [ ] Add/remove a menu item from the `ui/user_menu.rs` popup itself
      (`Ins`/`Del`-shaped bindings?) instead of hand-editing
      `LitastumMenu.toml` in the built-in editor -- the actual reason
      the format switched to TOML; not implemented yet.

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
