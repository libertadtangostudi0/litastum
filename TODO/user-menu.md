# User menu (F2)

See `TODO/user-menu-spec-symbols.md` for the full special-symbols
reference table (Far's `!...!` family and litastum's own `{{...}}`
family) -- this file stays the running feature/design log.

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
- [x] `!&` and `!?Label?Default!` work, matching real Far Manager's own
      macros -- superseded below by the fuller macro engine, but kept
      here for history.
- [x] **Full Far `!...!` macro set, plus a new litastum-native `{{...}}`
      family** (`parse::substitute_macros`/`consume_far_token`/
      `consume_litastum_token`) -- requested directly: reading real
      Far syntax properly (not just the two macros first implemented),
      *and* a high-level syntax of litastum's own that doesn't collide
      with `cmd`/PowerShell/`sh`'s own special characters the way Far's
      `!...!` genuinely does (cmd's `!VAR!` delayed-expansion syntax
      uses the exact same delimiter). Confirmed against Far's own
      `@MetaSymbols` help topic (`FarEng.hlf.m4`), not guessed:
      - `!.!`/`!`/`` !` ``/`!~`/`` !`~ `` -- cursor file name with
        extension, without extension, extension only (long/short
        variants of each).
      - `!-!`/`!+!` -- short name with extension (falls back to the
        long name -- see gaps below).
      - `!&`/`!&~[Q|q]` -- inline space-separated list of marked files
        (or just the cursor file if none are marked, same rule
        `Panel::marked_or_current` already uses for F5/F6/F8), quoted
        by default, `q` for unquoted.
      - `!@!`/`!$!` -- Far's "name of a file containing the list"
        (falls back to the same inline list `!&` produces -- see gaps).
      - `!:`/`!\`/`!/`/`!=\`/`!=/` -- current drive/path (symlink-
        resolved variants fall back to the plain path).
      - `!##`/`!^`/`![`/`!]` -- toggles which panel (passive/active/
        left/right) subsequent macros in the *same command* resolve
        against, exactly like real Far.
      - `!!` -- literal `!`.
      - litastum's own `{{cursor}}` (`!.!`'s equivalent) and
        `{{prompt:Label}}`/`{{prompt:Label:Default}}` (`!?Label?Default!`'s
        equivalent, going through the exact same `extract_prompts`/
        `substitute_prompts` path -- both syntaxes work in the same
        command, even mixed).

      **Gaps, all deliberate and documented at the code that hits
      them**: short (8.3) filename variants fall back to the long name
      (no Windows short-name lookup here, and no such concept at all on
      macOS/Linux); `!=\`/`!=/` fall back to the plain, unresolved path
      (no symlink-canonicalization helper yet); `!?!` (Far's separate
      per-file "description" feature) is left as literal text
      (litastum has no equivalent); `!@!`/`!$!` don't actually write a
      scratch file the way real Far does -- keeps `parse.rs` I/O-free,
      at the cost of that one narrow "avoid a command-line length
      limit" use case. `{{...}}` currently only covers the two macros
      actually asked for (`cursor`, `prompt`) -- more litastum-native
      equivalents (marked list, current path, ...) can follow the same
      `consume_litastum_token` pattern later if wanted.
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
- [x] **`F4` on a highlighted `Commands` item opens just that item's own
      command(s) in the real built-in editor** -- a scratch file
      (`state::create_command_edit_file`, one command per line) outside
      the project, opened via `Mode::Editing` exactly like any other
      file (real undo, syntax highlighting, multi-line editing), not a
      bespoke UI form. `app.user_menu_command_edit` (same
      "park the menu, hand it back once the editor really closes"
      shape as `app.editor_return_to`) carries the menu alongside the
      scratch path; `editor_keymap::return_from_editor` finishes the
      session (`state::finish_command_edit`) once the editor actually
      closes -- reads the scratch file's current lines back into the
      item's commands, persists, deletes the scratch file. A no-op on a
      `Submenu` item (nothing single to edit there).

      This landed after two rejected attempts, both reported directly:
      the first opened the *entire* `LitastumMenu.toml` in the built-in
      editor ("хочется редактировать не весь конфиг, а только команду/ы
      внутри элемента" -- not the whole config, just the item's own
      command(s)); the second, misreading that as "avoid the file
      editor entirely," built a bespoke single-line popup form instead
      -- also wrong ("редактирование должно быть в редакторе" -- it
      has to be the real editor, just scoped to this one item, not the
      whole file). Both were removed rather than kept alongside this.
- [x] **`Right`/`Left` work as navigation alternates for `Enter`/`Esc`**
      on the menu -- `Left` backs up one level or closes the menu at the
      top level, identical to `Esc` in every case. `Right` descends into
      a submenu identical to `Enter`, but on a `Commands` item
      deliberately does *not* run it the way `Enter` does -- an earlier
      version made `Right` behave identically to `Enter` there too,
      reported directly as a real hazard: simply arrowing through the
      menu could fire a real command. `Right` on a `Commands` item now
      opens it for editing instead (the exact same flow `F4` uses,
      `input::open_selected_item`/`open_edit_selected_command`) --
      `Enter` is the *only* key that actually runs anything.

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
      no `!&`/`!?Label?Default!` authoring help. `F4`'s edit-in-editor
      flow does support multiple command lines (one per line in the
      scratch file), unlike `Ins`'s own single-command-only shape --
      still no hotkey field there either. Still easiest to author a
      hotkey by hand-editing `LitastumMenu.toml` directly.
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
