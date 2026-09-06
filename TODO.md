# TODO
sjflsdfk
Near-term, actionable items. Full staged plan:
`.claude/rules/litastum-roadmap.md`. Design for the items below:
`ARCHITECTURE.md`.

## Stage 3 UI pass (in order — see ARCHITECTURE.md "suggested implementation order")

- [x] `theme.rs` — color constants from `.claude/rules/litastum-ui-theme.md`
- [x] Column-major `Panel` navigation (`columns` field, row/col index math)
- [x] `ui.rs`: replace `List` with a manual column-major grid renderer
- [x] `keymap.rs` + `command.rs` — extract key resolution/execution out of `main.rs::handle_event`
- [x] Wire Left/Right column movement through the new command layer

## Built-in editor (F4, `editor.rs`, `edtui`) — MVP + syntax highlighting landed, gaps left

- [x] Confirm-before-discard prompt when closing (`Esc`) with unsaved
      changes — `Mode::ConfirmDiscard`, `editor_keymap::resolve_confirm_discard`
- [x] Syntax highlighting (`edtui`'s `syntax-highlighting` feature,
      `syntect`-backed) — switched engines from `tui-textarea` to
      `edtui` with our own non-modal keymap to get this; see
      [[litastum-stack]] for the full story and lessons learned
- [x] Line numbers, on by default — `EditorView::line_numbers(LineNumbers::Absolute)`,
      gutter themed via `EditorTheme::line_numbers_style` (`theme.text_dim`
      on `theme.bg`) instead of `edtui`'s own hardcoded black/gray
      default. Absolute, not relative — this is a standard (non-modal)
      editor, not a vim-style one where relative numbers help with
      motion counts
- [ ] Handle non-UTF-8 / binary files without just silently doing
      nothing on F4 — at least a status-bar message once one exists
- [ ] Ctrl+V over an active selection doesn't replace it (clears the
      selection, then pastes at the cursor instead) — `edtui`'s real
      "paste over selection" action isn't publicly exported; see
      [[litastum-stack]]
- [ ] Undo is per-character (`capture_on_insert: true`), not grouped by
      typing burst like most editors — `EditorState::capture` being
      crate-private forecloses implementing our own grouping; see
      [[litastum-stack]]
- [ ] `onig` (a C library, via `syntect`'s default features) is now a
      transitive dependency — built fine locally, but is this project's
      first non-pure-Rust dependency; watch for build issues on
      environments without a C toolchain — see [[litastum-stack]]
- [x] Syntax highlighting for `.ps1`/`.psm1`/`.psd1` (PowerShell) —
      `syntect`'s bundled default set doesn't include PowerShell at all
      (confirmed by `editor::tests::syntect_bundles_rust_but_not_powershell`;
      `.rs`/Rust *is* bundled, so this wasn't our extension-lookup logic
      being wrong). Fixed by bundling our own grammar at compile time
      (`assets/syntax/PowerShell.sublime-syntax` — from
      github.com/SublimeText/PowerShell, MIT license, see
      `assets/syntax/PowerShell.LICENSE.txt`) and loading it into a
      second, minimal `SyntaxSet` via `SyntaxSetBuilder`
      (`editor.rs::bundled_extra_syntax_set`), since `syntect` only
      loads the YAML `.sublime-syntax` format itself — its `plist-load`
      feature covers `.tmTheme` *color themes*, not `.tmLanguage`
      *grammars*, which is why the obvious first choice
      (github.com/PowerShell/EditorSyntax, `.tmLanguage`-only) turned
      out to be a dead end and had to be swapped out
- [x] Syntax highlighting for `.ini`/`.cfg`/`.conf` — same underlying
      gap, but this time confirmed missing from sublimehq/Packages
      itself (not just `syntect`'s build of it — the upstream source
      genuinely has no INI syntax). Same fix, same mechanism
      (`bundled_extra_syntax_set` now holds all the bundled grammars):
      `assets/syntax/INI.sublime-syntax`, from
      github.com/jwortmann/ini-syntax (Apache-2.0 license, see
      `assets/syntax/INI.LICENSE.txt`). Its own `hidden_file_extensions`
      also covers `.editorconfig` and a handful of other INI-shaped
      dotfiles for free
- [x] Syntax highlighting for `.toml`/`Cargo.lock`, `.gitignore`,
      `.gitattributes` — genuinely present in sublimehq/Packages
      (confirmed by browsing the repo directly) but, unlike almost
      everything else there, not included in `syntect`'s own default
      bundle for some unknown reason. Pulled `TOML.sublime-syntax`/
      `Git Ignore.sublime-syntax`/`Git Attributes.sublime-syntax`
      straight from that same repo (permissive license, see
      `assets/syntax/sublimehq-Packages.LICENSE.txt` — the exact source
      `syntect`'s own default set is already built from, so no new
      licensing question). Also fixed a real bug found along the way:
      `Editor::view` only ever looked up a highlighter by
      `Path::extension()`, which returns `None` for dotfiles like
      `.gitignore` (Rust treats a leading dot with no further dot as
      "no extension") — so those never even reached a highlighter
      lookup at all, regardless of what grammars were bundled. Now
      tries the full file name first, then the extension, matching
      `syntect`'s own `SyntaxSet::find_syntax_for_file` convenience
      lookup. Second bug found by hand right after, testing the fix
      above in the real app: `.gitignore` opened and a highlighter
      *resolved* (name-based lookup returned `Some`), but rendered with
      zero color — Git Ignore's/Git Attributes' grammars both
      `include:` rules from a separate, shared `Git Common.sublime-syntax`
      (`hidden: true`) that hadn't been bundled alongside them, so every
      `include:` silently resolved to nothing (`syntect` doesn't treat
      an unresolved include as a load error, so nothing failed loudly).
      Fixed by bundling `Git Common.sublime-syntax` too; caught for
      real this time by a test that actually runs highlighting and
      checks a comment line gets colored, not just that a
      `SyntaxHighlighter` was constructible
      (`editor::tests::gitignore_comments_are_actually_colored_not_just_resolvable`)
- [x] Syntax highlighting for `.git/config` (and, generally, any
      bundled grammar that identifies itself by *content* rather than
      name) — reported after `.gitignore`/`.gitattributes` above, and
      explicitly asked to be solved generally rather than one file at a
      time. `.git/config` has no usable name (`file_name` is just
      `"config"`) or extension, but `GitConfig.sublime-syntax` (also
      pulled from sublimehq/Packages) declares
      `first_line_match: ^\[core\]` for exactly this reason — so
      `editor.rs::resolve_syntax_highlighter` grew a third lookup tier,
      tried only when nothing matched by name: `.first_line`
      (captured once at `Editor::open`) against `syntect`'s own bundled
      set, then ours, mirroring `syntect`'s own
      `SyntaxSet::find_syntax_for_file` convenience method. This is the
      "universal" half of the fix — any future grammar (bundled or
      `syntect`'s own) that leans on first-line detection now works
      without a one-off special case, not just Git Config
- [x] Syntax highlighting for `.md` under a *custom* `editor_theme` —
      `syntect`'s bundled Markdown grammar was always found fine (the
      highlighter really was running), but `scheme.rs::to_syntax_theme`
      only ever defined code-oriented scopes (keyword/string/comment/
      ...), so every `markup.*` scope Markdown actually emits
      (headings, bold, italic, lists, links, quotes, code spans) fell
      through to plain foreground — indistinguishable from "no
      highlighting" even though it technically wasn't that. The
      built-in `dracula` fallback theme (used with no `editor_theme`
      configured) already had real `markup.*` rules of its own, which
      is why this only showed up once a custom scheme was applied.
      Fixed by adding `markup.*` scope rules to `to_syntax_theme`,
      verified with a test that resolves the actual style via
      `syntect::highlighting::Highlighter` rather than just checking
      the scope list contains an entry

## RESOLVED: Ctrl+S / Ctrl+C / Ctrl+V / Ctrl+X (2026-09-04)

Confirmed working after splitting editor key resolution into
`editor_keymap.rs` (mirroring `keymap.rs`/`command.rs`) and adding
`logging.rs`. Root cause was never pinned down with certainty (the fix
landed together with the uppercase-letter match and the mode-borrow
restructuring in `main.rs::handle_editor_key`) — if a similar "key does
nothing" report comes up again, `logs/litastum.log` now has `debug!` on
every key event and resolved command in both modes, and `warn!`
specifically when `arboard::Clipboard::new()` fails, to make it
diagnosable without guessing.

The logging infrastructure stays (`logging.rs`, size-capped at 30 MB —
see `SizeCappedFile`) for the next time something like this happens.

## Theming (`scheme.rs`, `config.rs`) — landed, gaps left

Windows Terminal-format color schemes drive the app's interface
(panels, borders, F-key bar, ...) and the editor's syntax highlighting
*independently* — `config.json`'s `interface_theme` and `editor_theme`
keys, matching how Far Manager itself keeps those two systems separate.
See [[litastum-theming]] for the full design and the mapping
conventions.

- [x] F9 → Options → Color schemes in-app theme picker (`theme_menu.rs`,
      reached through `menu.rs`) — lists `themes/*.json`, applies live
      (no restart) and persists to `config.json`. See "F9 menu" below
      for how much of a real top-menu bar this actually is (not much)
- [ ] No hot-reload when a theme *file itself* is edited on disk while
      running (roadmap stage 5 territory, `notify` crate) — F9 re-scans
      the `themes/` directory each time it opens, but doesn't watch it
- [ ] Real terminal cursor *color* isn't driven by `cursorColor` (only
      shape is themed) — needs an OSC 12 escape sequence crossterm
      doesn't wrap
- [x] File-type coloring (`panel.rs::HighlightRole`) — archives,
      executables/scripts, and VCS metadata dirs (`.git`/`.svn`/`.hg`/
      `.bzr`) get distinct colors, Far Manager-style; reverses an
      earlier "no file-type color dots" decision (see
      [[litastum-theming]]) per explicit later request. Ordinary
      directories are *not* colored — checked against a real Far
      screenshot, only VCS dirs actually stood out there. Added
      `Theme::success`/`warning` (from the original "color 2" plan,
      only `danger` had actually landed before) to drive it
- [ ] `ColorScheme`'s `black`/`white`/most `bright*` fields are parsed
      but not yet consumed by `to_theme`/`to_syntax_theme` — kept for
      format fidelity and future mapping expansion, same situation as
      `Entry::size`/`modified` below; `#[allow(dead_code)]` on the
      struct silences the warning deliberately rather than dropping
      the fields

## F9 menu — Main → Commands/Options landed, real menu still to do

- [x] `menu.rs` — F9 opens a top menu (`Main`: `Commands`, `Options`)
      that descends into a submenu instead of jumping straight to a
      leaf action. `Esc` backs up one level at a time rather than
      always closing outright. Dispatch on `Select` is matched on
      `(level, item label)` rather than a positional index, so
      reordering `MenuLevel::items` can't silently wire the wrong
      action to a key.
- [x] `Options` → `Color schemes` (`Mode::ThemeMenu`, unchanged) and
      `Save setup` — Far Manager's own Shift+F9, reached only through
      the menu here (no global hotkey binding, wasn't asked for):
      persists the current session's choices to `config.json` on
      demand rather than every choice auto-persisting immediately the
      way the theme picker's own does. So far, that's just the active
      shell profile (`config.rs::save_setup`/`load_active_shell`,
      applied back at startup in `main.rs` if the saved name still
      matches a built-in profile) — the one setting that wasn't already
      being persisted somewhere.
- [x] `Commands` → `Find file` and `History` — see their own sections
      below.

`menu.rs` is deliberately *just* enough structure to reach what's
actually been asked for so far — not a real F9 top-menu bar (Far
Manager's own F9 is Left/Files/Commands/Options/View/Right, each with
real submenus of their own, and `Commands`/`Options` here only have two
items apiece so far). Gaps if this grows toward that:

- [ ] `MainMenu::back()` hardcodes every non-`Main` level's parent as
      `Main` — fine while the menu stays two levels deep, would need
      each level to know its own parent if a third level is ever added
- [ ] No keyboard shortcut letters (Far-style `S` for Settings, etc.) —
      `Up`/`Down`/`Enter` only
- [ ] Ctrl+V-over-selection replacing text, syntax highlighting for a
      *currently open* editor when its theme changes live — out of
      scope for the picker itself, listed here only because "apply
      live" doesn't retroactively re-theme an already-open `Editor`
      (the new `editor_theme` applies to the next file opened)
- [ ] No preview while browsing the theme list — colors only change
      once applied (Enter/I/E), not as you move the cursor over each
      name
- [ ] Long theme lists aren't scrolled, just clamped to the terminal
      height — fine for a handful of files, not for many

## Find file (`find_file.rs`) — F9 → Commands → Find file, landed, gaps left

Type a filename substring, `Enter` searches recursively from the active
panel's directory, `Enter` on a result moves the active panel there
with the file selected. Far Manager's own Alt+F7 — reached only through
the menu here, no global hotkey (wasn't asked for).

- [x] Recursive substring search (`find_file::search`, case-insensitive,
      matches directory names too, not just files), capped at 200
      results / 50,000 visited entries so a huge tree (a repo's own
      `target/`, `.git/`, `node_modules/`, ...) can't hang the UI
      indefinitely — deliberately no directory exclusions beyond that
      cap, a plain substring search same as Far's own "Find file"
      starts as
- [x] Glob patterns (`*`/`?`, e.g. `*.md`) — reported as a real bug
      almost immediately: `*.md` was being searched for as the
      *literal* six-character substring `"*.md"`, which matches no
      real file name. `find_file::matches_query` now switches to
      `glob_match` (a hand-rolled, classic greedy two-pointer `*`/`?`
      matcher — no `[...]` character classes, no escaping) whenever the
      query actually contains a wildcard character; a plain query with
      neither still matches by substring, so "just type part of the
      name" keeps working without forcing `*name*` on every search
- [ ] No exclusion of `.git`/`target`/`node_modules`/... by default —
      relies entirely on the visited-entry cap to stay responsive in a
      big tree, rather than skipping obviously-uninteresting
      directories up front
- [ ] Search runs synchronously on the key-handling thread — blocks
      the UI (no spinner, no cancel) until it finishes; fine for the
      repo sizes tried so far, would need a background thread for a
      truly huge tree
- [ ] No content search (Far's own Alt+F7 can also search *inside*
      files) — file names only
- [ ] Results aren't scrolled, just clamped to the terminal height

## History (`command_line.rs`) — F9 → Commands → History, landed, gaps left

Every command actually run from the command line is recorded
(`command_line::record_history`, `App::command_history`, capped at 50,
skips an immediate repeat) — F9 → Commands → History lists them,
`Enter` copies the highlighted one into the command line (doesn't
auto-run it — recalling something to tweak before running felt like
the more common case, and never auto-executing is the safer default
regardless), `Esc` cancels. Far Manager's own Alt+F8 — reached only
through the menu here, no global hotkey, and deliberately *not*
`Up`-arrow recall: arrows stay bound to panel navigation on the
always-live command line (see [[litastum-command-line]]), which is
exactly why a menu-driven popup was the way to add history at all
without reopening that conflict.

- [ ] Session-only — not persisted to `config.json`, resets to empty
      every run
- [ ] No search/filter within a long history list — just `Up`/`Down`
- [ ] `cd`s are recorded like any other command, `cls`/`clear` are too
      — no filtering of "boring" entries
- [ ] Results aren't scrolled, just clamped to the terminal height

## Command line (`command_line.rs`, `shell.rs`) — landed, gaps left

Always-live Far Manager-style command line with `cd` special-casing and
a `Ctrl+P` shell-profile picker (PowerShell/Command Prompt today). See
[[litastum-command-line]] for the full design and every scope cut below
in more detail.

- [ ] No cursor movement within the typed command (append/backspace
      only) — arrows are needed for panel navigation even while typing,
      so they can't double as text-cursor movement without real
      ambiguity
- [ ] No `Up`-arrow history recall — same reason, arrows are taken; see
      "History" above for the menu-driven way around this instead
- [ ] Bare `cd` (no argument) is a no-op, not "go to home directory"
- [x] `Tab` completes the last typed word as a filesystem path
      (`command_line::complete`) instead of always switching panels —
      reported as a bug: `Tab` used to hit `keymap::resolve`'s
      Tab-as-`ToggleActive` binding unconditionally, even mid-command,
      which is backwards from every shell's own convention for the key.
      Now special-cased ahead of that table (same pattern as `Ctrl+P`)
      whenever the command line has something typed; falls through to
      the usual panel-switch once it's empty. One match completes fully
      (trailing separator for a directory, trailing space for a file);
      several matches enter a `Tab`-cycling session
      (`App::command_line_completion`, a `command_line::CompletionCycle`)
      — each further `Tab` steps to the next match, wrapping back to
      the first after the last, `cmd.exe`'s own convention (explicitly
      requested over completing to the matches' shared prefix and
      stopping there); any other edit to the line ends the session.
      None leaves the line untouched. No completion for command *names*
      themselves (`PATH` scanning), only paths — same scope as the
      existing `cd` handling
- [x] `cls`/`clear` are special-cased like `cd` — `terminal.clear()`
      directly, no subprocess, no TUI suspend at all. Found by actually
      running `cls`: shelling out to a real `cls` wiped the `"{cwd}>
      cls"` prompt line we print for every command, leaving just the
      "Press any key to continue..." pause floating on an otherwise
      blank screen — technically working as designed, but looked like
      a broken/blank screen, reported as one
- [ ] Shell profile picker has no Git Bash/WSL/pwsh/Azure Cloud Shell
      entries (unlike the Windows Terminal dropdown that prompted this
      feature) — only `cmd`/`powershell` (Windows) or `$SHELL`/`sh`
      (Unix), which are universally present so need no detection.
      Adding the others needs real `PATH`/install-dir probing, not done
- [ ] Shell profile choice isn't persisted to `config.json` — resets to
      the platform default every run (`config.rs` already has the
      persistence pattern from the theme picker, if this is wanted)
- [ ] Opening the editor (F4) or another popup (F9, `Ctrl+P` itself)
      while text sits in the command line doesn't warn about it — the
      text is preserved and still there afterward, just easy to forget
      about since nothing currently calls it out

## Copy / Move (`F5`/`F6`) — landed, gaps left

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
- [ ] No multi-select — same gap as delete (F8), only ever acts on the
      entry currently under the cursor
- [ ] No overwrite confirmation — an existing file/directory at the
      destination is silently replaced (`fs::copy`/`fs::rename`'s own
      behavior), unlike Far Manager's own "already exists, overwrite?"
      prompt
- [ ] No progress indicator for a large copy/move — the prompt just
      sits there (frozen, no visible progress) until the whole
      operation finishes; fine for small files, not for a big tree
- [ ] A failed transfer is only logged (`debug!`), not shown to the
      user — same status-bar gap as delete

## Delete (`F8`) — landed, gaps left

Confirm-before-delete, same shape as the editor's `ConfirmDiscard`
prompt: `F8` opens `Mode::ConfirmDelete(PendingDelete)` instead of
deleting immediately, `Y` deletes (`fs::remove_file` for a file,
`fs::remove_dir_all` for a directory — recurses without asking twice,
matching Far Manager's own F8), `N`/`Esc` cancels with nothing touched.

- [x] Confirmation prompt + actual delete, single entry under the
      cursor (`keymap.rs::ConfirmDeleteCommand`,
      `main.rs::handle_confirm_delete_key`)
- [ ] No multi-select — Far Manager lets you mark several entries
      (`Ins`) and delete them together; this only ever acts on the
      entry currently under the cursor
- [ ] A failed delete (permissions, file in use, ...) is only logged
      (`debug!`), not shown to the user — no status-bar message surface
      exists yet (same gap as the non-UTF-8-file case above)
- [ ] No "move to Recycle Bin" option — always a hard, permanent delete

## Next up

- [ ] Show item count / free space in each panel's footer (mockup has
      this; not yet in `Panel`/`ui.rs`)
- [ ] Use `Entry::size`/`Entry::modified` for something, or drop them —
      `#[allow(dead_code)]` on `Entry` silences the warning deliberately
      in the meantime, not a fix in itself
- [x] `Alt` swapping the F-key hint bar to a second row of labels — the
      earlier bullet here guessed this meant a console/"additional
      screen" toggle; a follow-up screenshot clarified it's Far
      Manager's actual bottom bar changing labels while `Alt` is held.
      Landed as `App::alt_held` (set from every key event's modifiers
      in `main.rs::handle_event`) driving `ui::draw_function_keys`'s
      choice between `DEFAULT_LABELS`/`ALT_LABELS`, plus the one label
      that's an actual rebinding rather than cosmetic: `Alt+F7` opens
      Find file directly (`command_line.rs`, same special-casing
      pattern as `Shift+F6`), matching real Far's own global shortcut
      for it — previously only reachable through F9 → Commands → Find
      file. **Known limitation, not a bug**: terminals (including the
      Windows Console API this project mainly targets) generally don't
      deliver a standalone press/release event for a bare modifier key
      on its own — only modifier flags riding along with an actual
      keypress. So the alt row appears the instant `Alt+`-something is
      pressed, but only reverts on the *next* key event without `Alt`,
      not the instant `Alt` alone is released (`App::alt_held`'s own
      doc comment has the full explanation). No test coverage added —
      both the modifier-tracking assignment and the `Alt+F7` branch
      live in code that already has no unit tests for the same reason
      (`main.rs::handle_event`/`command_line.rs::handle_browsing_key`
      need a real `Terminal`)
- [ ] `Alt+F1`/`Alt+F2` — Far Manager's own per-panel drive-switching
      menu: pressed on the left/right panel respectively, pops up a
      list of available drives (on Windows, logical drive letters —
      `C:`, `D:`, ...) and navigates that panel to the picked one's
      root. Requested explicitly; not designed yet — needs a
      cross-platform way to enumerate drives (Windows-only concept as
      such; a Unix equivalent would be closer to mount points, out of
      scope for a first pass)

## Housekeeping

- [ ] `Cargo.toml`: replace `YOUR_USERNAME` placeholder in `repository`
