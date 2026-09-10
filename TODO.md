# TODO

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
- [ ] **On the horizon, not scoped yet**: an F9 menu inside the editor
      itself (`Mode::Editing`'s own F9, distinct from the browser's
      `theming::MainMenu` — Far Manager keeps a separate per-context F9
      too). Raised as a home for a few possible settings, floated but
      not committed to: re-interpreting the already-open buffer's bytes
      under a different codepage (lighter-weight than a full save-in-
      arbitrary-encoding pipeline — decode again from the bytes already
      read, via `encoding_rs`, rather than a round-trip converter);
      toggling visibility of line-ending/whitespace markers (exact scope
      still undecided — CRLF/LF glyphs at end of line vs. VS Code-style
      "render whitespace" for trailing spaces/tabs, or both); "possibly
      something else," not yet named. Don't start implementation from
      this bullet alone — needs its own scoping pass (new `Mode`, key
      dispatch, menu rendering, and the actual encoding/whitespace logic
      each item implies) before any of it is built.
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
- [x] Syntax highlighting for `CMakeLists.txt`/`.cmake`, and for this
      project's own `CMakeLists.txt.sdk` build-template convention —
      Sublime Text has never shipped CMake support by default (a
      third-party package there), so `syntect`'s own bundle lacks it
      too; fixed the same way as PowerShell/INI, bundling
      github.com/zyxar/Sublime-CMakeLists's grammar (plus its hidden
      `CMakeCommands.sublime-syntax` include dependency — see
      [[litastum-theming]]'s "Syntax highlighting" section). The `.sdk`
      suffix itself is handled in `Editor::view`, not the grammar: it
      retries the same name/extension lookup with one trailing `.sdk`
      stripped, a general mechanism rather than a CMake-specific hack
- [ ] Rust (`.rs`) highlighting still uses `syntect`'s own bundled
      default grammar — tried swapping in github.com/rust-lang/
      rust-enhanced (a more detailed community `.sublime-syntax`) to get
      closer to what VS Code + rust-analyzer shows, including two local
      patches (an ordinary type name like `PathBuf` used as a plain
      field/parameter type got no color at all; primitive types like
      `bool` shared a literal scope with the `let`/`const`/`static`
      keywords, so no theme could color them differently) — reverted
      anyway, didn't hold up well enough against real-world comparison
      to be worth keeping. Note: rust-analyzer's own VS Code extension
      ships *no* grammar of its own (confirmed directly, its
      `package.json` has no `contributes.grammars`) — it's a pure LSP
      client layering real *semantic* tokens over VS Code's own built-in
      Rust grammar, which is itself a `.tmLanguage.json` (plist) file
      `syntect` can't load at all (same "`.tmLanguage`, not
      `.sublime-syntax`" wall PowerShell hit — see above). So "just get
      VS Code's real grammar" isn't actually on the table either way.
- [ ] **Evaluate**: is writing our *own* `.sublime-syntax` Rust grammar
      from scratch (rather than adopting/patching an existing one, per
      the above) worth doing at all — scope it out before committing to
      it:
      how much of `syntect`'s existing bundled Rust grammar is actually
      already fine vs. genuinely under-highlighting; how far a
      hand-written grammar could realistically get without drifting
      into reimplementing a real parser; whether the two local-patch
      lessons above (type-position CamelCase heuristic, primitive types
      needing their own scope distinct from keywords) are cheap wins
      worth folding into a fresh grammar or symptomatic of a much bigger
      gap. Purely a sizing/scoping task — no grammar work until this is
      done and reviewed.
- [x] Highlight every other occurrence of the identifier currently under
      the cursor, VS Code-style (`editor/word_highlight.rs`). Turned out
      not to need a hand-rolled render pass at all -- `edtui`'s own
      `EditorState::highlights` field (`Highlight::new(start, end,
      style)`) is already rendered every frame, layered exactly where
      this needs it ("selection takes priority, then highlights, then
      base", confirmed directly from `edtui`'s `view/internal.rs`), so
      `Editor::view` just recomputes it fresh each frame from the
      cursor's current position. A `Highlight`'s style fully *replaces*
      whatever span it lands on (no fg/bg merging), so highlighted
      occurrences lose their own syntax color and render in one flat
      color — the same tradeoff this codebase's own text-selection
      highlighting already makes. Colored via `theme.text` on
      `theme.border` (reused, not a new `Theme` field, so it's
      automatically theme-dependent as requested), and skipped entirely
      while a selection is active (matching VS Code — "the word under
      the cursor" isn't coherent mid-selection). Word-boundary detection
      is a small local `is_word_char` (ASCII alphanumeric or underscore,
      matching `edtui`'s own internal `CharacterClass::Alphanumeric`) —
      no need to reach into the crate's `pub(crate)` `CharacterClass`
      for something this simple, unlike `bindings::word_select`'s own
      history.
- [ ] In-editor find (`Ctrl+F`/`F7`, Far Manager's own editor
      convention — not to be confused with the file-panel's own F7/
      Alt+F7 "find file[s]"/"find file *content*" above, an entirely
      different, already-landed feature that searches *across files* in
      the panel, not *within* the currently open one). Needs: a search
      popup/prompt for the query (reuse `ui/popup.rs`'s shared chrome,
      per [[litastum-popup-design]]), a plain substring or (stretch)
      regex scan over `state.lines`, highlighting each match (possibly
      sharing groundwork with the "highlight every occurrence of the
      word under the cursor" item above, if that lands first), jump-to-
      next/previous-match navigation (`F3`/`Shift+F3`, or Far's own
      repeat-last-search convention), and scrolling the match into view.
      `edtui` has no built-in search of its own to lean on — this would
      be hand-rolled, same as the word-selection logic elsewhere in this
      file.
- [ ] **Multi-line syntax constructs (block comments, multi-line
      strings, ...) don't highlight correctly past their own opening
      line — a real, confirmed `edtui` 0.11.7 architecture limitation,
      not a bug in any one bundled grammar.** Reported directly against
      a Groovy/Jenkinsfile banner comment (`/******...` opening a block,
      several `***`-only lines, `...******/` closing): only the opening
      line rendered as a comment; every line after it rendered as
      ordinary code. Traced to the actual dependency source
      (`edtui-0.11.7/src/view/syntax_higlighting.rs::SyntaxHighlighter::highlight_line`
      and `src/view/internal.rs::line_into_highlighted_spans_with_selections`,
      confirmed by reading both directly, not guessed): `highlight_line`
      takes `&self` (no mutable state) and constructs a **brand new**
      `syntect::easy::HighlightLines` from scratch on *every single
      call*, and `EditorView`'s own rendering calls it once per visible
      *row*, independently, with no `ParseState`/`HighlightState` ever
      threaded between rows. Confirmed this is genuinely `edtui`'s own
      limitation, not a grammar issue: a standalone test driving the
      *same* bundled Groovy grammar through one shared, persistent
      `HighlightLines` instance across multiple `highlight_line` calls
      (`editor::syntax::tests::groovy_banner_comment_colors_every_line_as_comment`)
      colors every line correctly — the grammar and `syntect` are fine
      on their own; `edtui`'s own per-row call pattern is what throws
      the state away. Affects **every** language with any multi-line
      construct, not just Groovy — C/Rust/C++ block comments, Python/
      JS-style multi-line strings, etc. are presumably equally affected,
      just less commonly written in a way that's visually obvious as
      "broken" (most everyday `/* ... */` comments people actually write
      are short and often single-line in practice, so this may simply
      not have been noticed yet for other languages). Already on
      `edtui`'s latest published version (0.11.7, confirmed via
      crates.io) — no version bump fixes this. No workaround available
      from litastum's side without either patching a vendored/forked
      copy of `edtui` (`SyntaxHighlighter::highlight_line` would need a
      `&mut self` + persisted `HighlightState`/`ParseState`, and
      `EditorView`'s own render loop would need to call it top-to-bottom
      in row order, never out of order or for only a scrolled subset,
      for the state to stay meaningful) or replacing `edtui`'s syntax-
      highlighting feature with a hand-rolled rendering pass entirely —
      both large undertakings, not scoped further here. **Reported
      upstream**: github.com/preiter93/edtui/issues/74 — waiting on a
      response before deciding whether to fork/patch `edtui` ourselves
      or wait for an upstream fix.

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

Type a filename substring or glob, `Enter` searches recursively from
the active panel's directory, `Enter` on a result moves the active
panel there with the file selected. Far Manager's own Alt+F7 — reached
through the menu *and* bound directly (`command_line.rs`), matching
real Far.

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
- [ ] **Flagged performance risk, not yet a reported problem**:
      `find_file/search.rs`'s recursive walk is synchronous, single-
      threaded, and runs directly on the key-handling thread — no
      spinner, no cancel, and no default exclusion of `.git`/`target`/
      `node_modules`/... (the only safety net is the `MAX_RESULTS`/
      `MAX_VISITED` visited-entry cap, which bounds the damage but
      doesn't make a big search *fast*). Fine for every repo size tried
      so far; revisit if a genuinely large tree makes this noticeably
      slow or freezes the UI — see `search.rs`'s own doc comment for
      the fix directions (ignore-pattern pruning, a background thread
      with cancel, or parallelizing the walk)
- [ ] No content search (Far's own Alt+F7 can also search *inside*
      files) — file names only
- [ ] Results aren't scrolled, just clamped to the terminal height
- [x] `Ctrl+S` on the results popup exports the full list (one full
      path per line) to a new file in the user's Downloads directory
      (`directories::UserDirs::download_dir()`, already a dependency)
      — requested explicitly. File name embeds the query and a
      hand-rolled `YYYY-MM-DD_HHMMSS` timestamp
      (`find_file::timestamp_for_filename`/`civil_from_days` — Howard
      Hinnant's public-domain days-to-civil-date algorithm, computed by
      hand from `SystemTime` rather than pulling in a date/time crate
      for one file name) so repeated exports never overwrite each
      other; the query is sanitized for Windows-illegal filename
      characters first (`*`/`?` show up constantly here, being glob
      syntax). Both the finding-Downloads half and the actual
      file-writing half are split apart
      (`export_results`/`write_results`) so the latter has real test
      coverage against a scratch directory — same reasoning as
      `config.rs`'s `set_interface_theme` vs. `try_persist` split, since
      the former goes through a real, un-injectable OS path. Success or
      failure both show a message under the results list (no other
      status-bar surface exists yet)

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
- [x] Text selection in the command line — `Shift+Left`/`Right`
      (character-wise) and `Ctrl+Shift+Left`/`Right` (word-wise) select,
      typing or `Backspace` over a selection replaces/deletes it, same
      shape as `text_field.rs`'s Copy/Move destination-field selection
      (which it directly reuses: `extend_selection_left/right`,
      the two new `extend_selection_word_left/right`, `delete_selection`)
      against a real cursor position the command line never had before
      (`App::command_line_cursor`/`command_line_selection_anchor`).
      Bare `Left`/`Right` are still panel-navigation-only, untouched —
      only the `Shift`/`Ctrl+Shift` combinations, which panel nav never
      claimed. Rendered with `theme.current_row_bg`, the same color the
      destination field's own selection already uses (not a distinct
      "Selected text" color reproducing Far's own olive-green group —
      judged not worth a dedicated `Theme` field for this one highlight,
      reusing the one that's already there for the same *kind* of
      selection elsewhere).
- [x] Plain `Ctrl+Left`/`Right` (no `Shift`) moves the command-line
      cursor by a word with no selection — `text_field::move_word_left/
      right`, clearing any active selection rather than collapsing to
      its edge (a real editor's Ctrl+arrow moves from the cursor and
      drops the selection, it doesn't jump to whichever edge is
      closer). Added right after the Shift-selection work above, once
      it was pointed out plain Ctrl+arrow cursor movement (no
      selecting) was still missing — `resolve(key.code)` only sees
      `KeyCode`, not modifiers, so before this Ctrl+Left/Right silently
      fell through to the exact same panel-navigation move as a bare
      arrow (not a behavior loss — bare arrows already did that).
- [ ] No `Up`-arrow history recall — same reason, arrows are taken; see
      "History" above for the menu-driven way around this instead. Requested
      directly: `Alt+Up`/`Alt+Down` to cycle through `app.command_history`
      while typing (bare arrows stay panel-navigation-only, unaffected) —
      `Alt` isn't otherwise claimed on these two keys, so this doesn't
      need the same workaround `Shift+F6`/`Ctrl+P` use to jump ahead of
      `keymap::resolve`'s table. Needs deciding how this interacts with
      the existing `Tab`-completion cycle (`App::command_line_completion`),
      which also reads `command_history` for its own separate cycling
      session.
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
- [ ] No persistent, resizable console/output area below the panels —
      real Far Manager keeps one, adjustable with `Ctrl+Up`/`Ctrl+Down`
      (shrinks/grows the panel area to reveal more or less of it) and
      scrollable on its own with `Ctrl+Shift+Up`/`Ctrl+Shift+Down`.
      Reported directly against the current design's real limitation:
      one command's output replaces the previous one's the instant the
      next command runs (full TUI takeover + "Press any key to
      continue..." pause, see `print_themed`'s own doc comment in
      `command_line/browsing.rs`) — there's no on-screen history of
      what earlier commands printed at all, let alone a resizable or
      scrollable view onto it. This needs real output *capture* (a PTY
      or at least a piped/buffered child process, keeping a scrollback
      buffer in `App`) instead of today's "suspend the TUI, inherit
      stdio directly, let the terminal itself render it" approach — the
      same architectural gap already called out for why the shelled
      command's own output can't be recolored either. A genuinely
      bigger redesign than the other command-line gaps above, not a
      small addition on top of the current model.

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

## Delete (`F8`) — landed, gaps left

Confirm-before-delete, same shape as the editor's `ConfirmDiscard`
prompt: `F8` opens `Mode::ConfirmDelete(PendingDelete)` instead of
deleting immediately, `Y` deletes (`fs::remove_file` for a file,
`fs::remove_dir_all` for a directory — recurses without asking twice,
matching Far Manager's own F8), `N`/`Esc` cancels with nothing touched.

- [x] Confirmation prompt + actual delete, single entry under the
      cursor (`keymap.rs::ConfirmDeleteCommand`,
      `main.rs::handle_confirm_delete_key`)
- [x] Multi-select — `F8` deletes every entry currently marked in the
      active panel (`Panel::marked_or_current`, shared with F5/F6's own
      `transfer_sources`) if any are, falling back to the cursor entry
      otherwise. `PendingDelete` now holds `entries: Vec<DeleteEntry>`
      rather than a single path/name/is_dir/size; a failure on one entry
      is only logged and doesn't stop the rest. The popup shows the
      name for a single entry or `"N items"` + the shared parent
      directory for several, mirroring the transfer popup's own
      single-vs-multiple wording
- [ ] A failed delete (permissions, file in use, ...) is only logged
      (`debug!`), not shown to the user — no status-bar message surface
      exists yet (same gap as the non-UTF-8-file case above)
- [ ] No "move to Recycle Bin" option — always a hard, permanent delete

## Next up

- [ ] Scrolling/pagination for the file panel's entry list — requested
      directly. Currently the 2-column column-major grid
      (`.claude/rules/litastum-ui-theme.md`) has no scroll offset at
      all, so a directory with more entries than fit the panel's own
      height is presumably just clamped/cut off rather than scrolling
      to follow the cursor — needs checking exactly what `ui.rs`'s
      current rendering does today before designing the fix. Likely
      needs a per-`Panel` scroll-offset field, kept in sync with cursor
      movement (`PageUp`/`PageDown` already exist as bindings elsewhere
      in this codebase — worth checking whether the panel honors them
      at all right now), and has to interact correctly with the
      column-major (fill column 1 top-to-bottom, then column 2) layout,
      not just a naive single-column scroll.
- [x] Multi-select in the file panel — landed as `Shift+A` (select all)
      and `Shift+Up`/`Down`/`Left`/`Right` (toggle-and-move, whole-
      column for Left/Right) rather than the `Ins`+`Shift+arrow`
      combination originally sketched here — see "Copy / Move" above
      for the full binding story and `panel/marks.rs`. Marked entries
      render in `theme.warning` (the "attention/marked" color the
      original UI-theme plan had already named but never wired up),
      overriding type-based coloring, rather than a distinct new
      `Theme` field.
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
- [x] `Alt+F1`/`Alt+F2` — Far Manager's own per-panel drive-switching
      menu (`explorer/drive_menu.rs`, `ui/drive_menu.rs`): `Alt+F1`
      always targets the left panel, `Alt+F2` always the right one
      (`DriveMenu::target_panel`, fixed at open time — independent of
      which panel currently has focus, matching real Far), lists
      logical drives with type and total/free space
      (`GetLogicalDrives`/`GetDriveTypeW`/`GetDiskFreeSpaceExW` via
      `windows-sys`, a new Windows-only dependency), `Enter` navigates
      via the existing `Panel::change_dir`. Windows-only real
      enumeration; the Unix build gets a single `"/"` entry rather than
      real mount-point enumeration — same scope cut as `shell.rs`'s
      Unix shell-profile fallback, not attempted here either
- [ ] **Diff view mode + conflict resolver** — placeholder, rules/scope
      to be filled in later (not yet specified: whether this is a
      standalone F-key-triggered mode, an editor overlay, how it hooks
      into VCS state if at all, two-way vs. three-way, resolution UI).
      Don't start implementation from this bullet alone. See "Git
      integration" below — likely related (a conflict resolver needs to
      know a file's conflicted-hunk structure from somewhere, which is
      git-specific).

## Git integration

Requested directly, three related but separable pieces — none scoped
in detail yet, don't start implementation from these bullets alone:

- [ ] **Core git mechanism** — how litastum actually talks to a git
      repo at all; needs deciding before either bullet below can be
      built on top of it. Options: shell out to the user's own `git`
      binary (simplest, matches this project's existing command-line
      shell-out pattern in `command_line.rs::run_command_line`, no new
      dependency, but output-parsing is inherently fragile against
      different git versions/locales), or a Rust library
      (`git2`/`libgit2` bindings — a C dependency, the same tradeoff
      `onig`/`syntect` already introduced, see
      `.claude/rules/litastum-stack.md`'s "Known risk" note — vs. pure-
      Rust `gix`, worth checking how complete its plumbing/porcelain
      coverage is for what's actually needed here before committing to
      it). Whichever is chosen, this is also the natural place to detect
      "is the current panel directory even inside a git repo" for
      showing/hiding any git-aware UI at all.
- [ ] **Dependency search by file path** — find what depends on (or is
      depended on by) a given file, scoped to the current repo. Scope
      still unclear: language-aware import/`use`-graph analysis (a much
      bigger undertaking, would need a parser per language) vs. a
      simpler text-based search for the file's own path/module name
      appearing elsewhere in tracked files (closer to what
      `find_file.rs`'s existing content-search machinery could be
      extended to do, see "Find file" above). Needs a decision on which
      of these is actually wanted before scoping further.
- [ ] **Commit search** — find commits by message/author/date/path,
      presumably via `git log` (or the equivalent plumbing call once the
      core mechanism above is chosen) with a filtering popup similar in
      shape to `find_file.rs`'s own search UI. Needs deciding: search
      scope (current file's history vs. whole repo), how results are
      presented/navigated (jump to a diff view — see the conflict-
      resolver placeholder above), and whether this needs its own
      history/persistence the way `command_line.rs`'s "History" feature
      does.

## Housekeeping

- [ ] `Cargo.toml`: replace `YOUR_USERNAME` placeholder in `repository`
