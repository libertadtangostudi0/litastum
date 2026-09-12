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
- [x] A `FarMenu.ini` found is *offered*, not read or converted
      silently (`Mode::ConfirmPortFarMenu`, `Y`/`N`) --
      `state::port_far_menu` does the actual DSL-parse-then-TOML-
      serialize once confirmed. Reported *even when `LitastumMenu.toml`
      already exists* (dropping a `FarMenu.ini` into an already-
      configured directory used to be silently ignored -- `resolve_menu`
      now checks for `FarMenu.ini` first, unconditionally), and checked
      once at startup too (`main.rs`, the panel's own starting
      directory), not only when `F2` happens to be pressed. `Y` backs up
      an existing `LitastumMenu.toml` to `LitastumMenu.toml.bak` before
      overwriting it, then moves `FarMenu.ini` itself to
      `FarMenu.ini.bak`; `N`/`Esc` still moves `FarMenu.ini` to that
      same backup name (without reading it) and shows a one-line
      `Mode::Info` popup saying where it went -- either answer has to
      move it out of the way, or its mere presence would re-trigger
      this same prompt on every future `F2`/startup check.
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

- [x] **Add/remove a menu item from the popup itself** (`Ins`/`Delete`),
      the actual reason the format switched to TOML above. `Ins` opens
      a small two-field form (title, then a single command -- leave the
      command blank to add an empty submenu instead of a leaf item);
      `Delete` removes the highlighted item immediately, no
      confirmation (a config file entry, not real user data). Both
      persist the *whole* tree back to `LitastumMenu.toml`.
      `UserMenuState` itself had to change shape for this: it used to
      clone each submenu's children into their own disposable level on
      `enter_submenu`, fine for read-only browsing but silently
      discarding any edit made three levels deep the instant `back()`
      popped that level. Rewritten to walk one single canonical tree
      (`root: Vec<MenuItem>`) through a path of indices instead, so an
      edit at any depth is automatically visible everywhere (including
      after persisting) with no separate sync step.

- [x] **`FarMenu.ini` encoding**: real Far Manager exports this file in
      UTF-16LE with a BOM (confirmed directly by inspecting a real
      exported file's raw bytes: `FF FE`, every ASCII character null-
      padded), not UTF-8 -- reported as "porting produces zero items"
      against an actual real-world file. `state::read_text_file_any_encoding`
      detects the BOM (UTF-16LE/BE, or a UTF-8 BOM) and decodes
      accordingly, falling back to plain UTF-8 when there's no BOM at
      all; `port_far_menu` reads through this instead of a bare
      `fs::read_to_string`, which fails outright on non-UTF-8 bytes.

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
- [ ] The `Ins` add-item form is deliberately minimal: no hotkey field,
      no multi-command items, no `!&`/`!?Label?Default!` authoring
      help. Still easiest to add those by hand-editing
      `LitastumMenu.toml` afterward (a submenu added via the form can
      be entered and built out with more `Ins`-added items, but a
      hotkey or a second command line on one of them needs the text
      editor).
- [ ] `Delete` has no confirmation and no undo -- accepted deliberately
      (see above), but worth revisiting if this ever needs to guard
      against a stray keypress the way F8's own delete does for real
      files.
- [ ] Long item lists aren't scrolled, just clamped to the terminal
      height, same gap `TODO/f9-menu.md` already lists for the F9 menu
      and color-scheme picker.
- [ ] No global (config-dir) or user-profile-level menu, only the
      per-directory one real Far Manager also supports -- not asked
      for, not attempted.
- [ ] Backups (`LitastumMenu.toml.bak`, `FarMenu.ini.bak`) use one fixed
      name, not a timestamped one -- porting twice in the same
      directory clobbers the previous backup rather than keeping both.
      Accepted deliberately for a rare, explicitly-confirmed action
      rather than reusing/generalizing `find_file/export.rs`'s own
      (differently-shaped) unique-filename timestamp logic.
- [ ] `Mode::Info` (the one-line dismiss-on-any-key notification) is
      new and deliberately generic, but currently has exactly one
      caller (the decline-and-backup path above) -- worth revisiting
      once/if a second use turns up, to see whether the generic shape
      actually holds up.
