# Built-in editor (F4, `editor.rs`, `edtui`) — MVP + syntax highlighting landed, gaps left

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
- [x] **A file with one pathologically long line made the editor
      visibly sluggish** — reported directly (a real file, essentially
      one enormous escaped log/diff dump with no real line breaks).
      Two independent per-frame costs both scale with a single line's
      length, and both run on *every* redraw (`main.rs::run`'s own
      per-event, no-batching architecture): `word_highlight.rs`'s
      "highlight every other occurrence of the word under the cursor"
      does a plain, un-indexed scan across every line, and `syntect`
      re-tokenizes a line's *full* text on every highlight pass
      regardless of how much of it is actually visible on screen.
      Fixed with one shared threshold,
      `word_highlight::MAX_HIGHLIGHTED_LINE_LEN` (20,000 -- double VS
      Code's own ~10,000-character tokenization cap, the same
      mitigation mainstream editors already use for this exact case):
      `word_occurrences` now skips any row past that length outright
      instead of scanning it, and `Editor::view` skips building a
      `SyntaxHighlighter` at all for the whole file if any line
      exceeds it (`has_pathologically_long_line`) -- a giant line makes
      syntax highlighting expensive on every frame regardless of which
      line is on screen, so disabling it file-wide is the correct
      scope, not just skipping the one offending line. Confirmed with
      a real rendering test (`a_pathologically_long_line_disables_syntax_highlighting_for_the_whole_file`)
      that no cell renders in a syntax color once the file has such a
      line, alongside a control case
      (`a_short_rust_file_does_get_real_syntax_coloring`) proving the
      same assertion would actually catch a regression, not just be
      trivially true.
      **Reported still laggy after this landed** — the fix above only
      addressed costs that scale with *syntax highlighting*; the actual
      complaint was specifically cursor movement itself feeling slower,
      which pointed at a third, unrelated
      O(line length) cost neither of the first two fixes touched:
      `Editor::is_dirty` used to recompare `state.lines != saved_snapshot`
      on every single call, and `ui/editor_pane.rs` calls it once per
      render frame for the "[modified]" title marker. Rust's derived
      `PartialEq` can only ever *disprove* equality early (the instant
      two rows differ) — *proving* equality, which is exactly what
      happens on every frame while the cursor is merely moving with no
      real edit (buffer content is genuinely unchanged), requires
      walking every character of every row. For an ordinary file this
      is unnoticeable; for the one enormous line that prompted this
      whole entry, it meant a full pass over hundreds of thousands of
      characters on every single arrow-key redraw — completely
      independent of syntax highlighting or word-occurrence scanning,
      which is why disabling those didn't help. Fixed by caching the
      comparison result (`Editor::dirty`) instead of redoing it on every
      `is_dirty()` call, recomputed by `input()` only for keys that
      could plausibly have mutated the buffer (`can_mutate_buffer`) —
      pure navigation (arrows/Home/End/PageUp/PageDown, any modifiers)
      now reuses whatever `dirty` already was rather than re-walking the
      whole buffer to reconfirm nothing changed. `select_all`/
      `extend_word_selection` bypass `input` entirely and never touch
      content either, so they correctly need no equivalent update.
      **Reported still laggy a second time even after this** — traced
      the remaining cost to `edtui` itself, not anything left in our own
      code. Generated real crate source with `cargo doc -p edtui
      --no-deps` (lands under this project's own `target/doc/`, so
      reading it doesn't cross the "stay inside the project folder"
      rule the way reading the raw `~/.cargo/registry` checkout would)
      and read `view.rs`/`view/internal.rs` directly: when there's no
      syntax highlighter (our own case, after the fix directly above),
      `EditorView::render` builds the line's display spans via
      `line_into_spans_with_selections`, which does
      `line.iter().skip(col_skips).enumerate()` -- iterating from the
      current horizontal scroll offset all the way to the line's own
      *end*, never clipped to the viewport's actual width. For an
      ordinary line this is unnoticeable; for one enormous line, this is
      an O(remaining line length) cost paid fresh on *every* redraw,
      independent of syntax highlighting (already disabled) and
      independent of `is_dirty` (already cached, see directly above) --
      genuinely the crate's own rendering, not reachable from any fix
      confined to this project's own source. (The syntax-highlighted
      path, `line_into_highlighted_spans_with_selections`, is worse
      still for the same file shape -- `line.iter().collect()`s the
      *entire* line into a `String` and runs `syntect` over all of it
      before cropping to the viewport only at the very end -- one more
      confirmation that disabling syntax highlighting for such files,
      above, was the right call and not merely cosmetic.)
      **Accepted as a known upstream limitation, not fixed** — asked
      directly, the options were: fork/vendor `edtui` and patch this one
      function to clip by viewport width (a real fix, but taking on
      ongoing patch-maintenance burden against future `edtui` upstream
      updates), degrade to a read-only/truncated view for such files
      (limits what the editor can actually do with them), or document
      the limitation and stop here — the last was chosen. A file
      containing one pathologically long line will still redraw slowly
      while the cursor is on-screen near it; genuinely fixing this needs
      an `edtui` change, not a litastum one.
- [x] **An F9 menu inside the editor itself landed**
      (`Mode::Editing`'s own F9, distinct from the browser's
      `theming::MainMenu` — Far Manager keeps a separate per-context F9
      too), scoped to exactly one setting, requested directly: switching
      between this project's own non-modal keymap and real Vim
      keybindings. `editor/keymap_mode.rs::EditorKeymapMode` (`Standard`/
      `Vim`, config.json-persisted the same way `PopupStyle` already is
      — `theming::config::{load_active_editor_keymap_mode,
      set_editor_keymap_mode}`) and `editor/keymap_menu.rs::EditorKeymapMenu`
      (a plain list picker, directly modeled on `popup_style_menu.rs`)
      are the new pieces; `Mode::EditorKeymapMenu(Editor, EditorKeymapMenu)`
      holds the editor being configured, same `ConfirmDiscard(Editor)`
      shape already used elsewhere for "pop a prompt over the editor,
      hand it straight back either way." `Vim` uses `edtui`'s own bundled
      `EditorEventHandler::vim_mode()` unmodified -- implementing real
      Vim bindings from scratch was never in scope, and `edtui` already
      ships one (`.claude/rules/litastum-stack.md` already noted this
      when picking `edtui` in the first place). Explicitly decided (asked
      directly, both confirmed): this project's own hand-rolled
      correction passes (`Editor::input`'s post-table block -- word-wise
      selection touch-tracking, `Shift`-arrow anchor fixes, line-boundary
      wrapping, ...) are specifically tuned against `Standard`'s own
      declarative table and are skipped entirely while `Vim` is active,
      rather than risking them running against Vim's own modal,
      multi-key sequences unverified; and the choice persists across
      restarts in `config.json`, matching `popup_style`'s own pattern
      rather than resetting per-session like the shell-profile picker
      does. `Editor::set_keymap_mode` live-switches an already-open
      session (rebuilds `event_handler`, resets `state.mode` to each
      keymap's own natural starting point -- `Standard` -> `Insert`,
      `Vim` -> `Normal`, matching real Vim's own convention -- and drops
      any active selection, since neither keymap's own selection
      semantics carry over meaningfully to the other) so applying a
      change from the menu needs no reopen. `F9` was previously a silent
      no-op while editing (not among the fourteen `crossterm::event::
      KeyCode` variants `edtui` itself understands, `edtui_supports_key`'s
      own doc comment) -- free real estate, not a rebind. Deliberately
      scoped to plain full-screen editing only: `F9` is a no-op while a
      linked Markdown preview session (`App::markdown_edit_preview`) is
      active, since `ui::draw` has no split-view rendering wired up for
      this new `Mode` and threading that through would have meant
      touching several more match arms for a case nobody asked for.
      Re-interpreting the buffer under a different codepage and toggling
      line-ending/whitespace markers were floated for this same menu
      earlier and are still just floated, not built -- either would need
      its own scoping pass, same as before.
      **Follow-up, requested right after landing**: `F9` originally
      jumped straight to the `Standard`/`Vim` picker -- restructured
      into a real (if currently one-item) submenu instead, `Editor::open`'s
      own new default confirmed unchanged at `Standard` (already the
      case, per `EditorKeymapMode::default()`, for new users with no
      `config.json` entry yet). New `editor/menu.rs::EditorMenu`
      (`ITEMS = &["Keybindings"]`, directly modeled on `theming::menu.rs`'s
      own `MainMenu`, just without its multi-level `MenuLevel` machinery
      since there's only one level here so far) and `Mode::EditorMenu(Editor,
      EditorMenu)` sit in front of `EditorKeymapMenu`: `F9` now opens
      `EditorMenu`, and `Enter` on `Keybindings` opens `EditorKeymapMenu`
      from there, unchanged in its own behavior otherwise. Both levels'
      own `Esc` close straight back to `Mode::Editing` rather than
      stepping back one level at a time -- matches `theming::PopupStyleMenu`'s
      own "leaf `Esc` closes all the way out" convention (reached via
      `MainMenu` -> `Options` -> `UI`, same shape), not `MainMenu`'s own
      multi-level `back()`, so there's no reason for the two menus in
      this app to disagree about it. Named `EditorMenu`/`" Menu "` (the
      inner picker's own title stays `" Keybindings "`) rather than
      something narrower, matching `theming::MainMenu`'s own naming and
      leaving room for the codepage/whitespace-marker ideas above to
      become a second item later without a rename.
      **Two real bugs found by hand while actually testing Vim mode
      afterward** (requested directly: test Vim behavior, find bugs),
      both fixed:
      1. `Ctrl+Shift+Left`/`Right` (`EditorCommand::WordSelect`) still
         ran `Editor::extend_word_selection` -- this project's own
         hand-rolled, `Standard`-keymap-tuned word-selection logic
         (`word_select_touch` state machinery built specifically around
         this app's own gesture, see that method's own doc comment) --
         completely regardless of `keymap_mode`, contradicting
         `EditorKeymapMode::Vim`'s own documented promise that none of
         this project's correction passes run while Vim is active.
         Unlike `Editor::input`'s own post-table block (already gated),
         this call site in `editor_keymap::handle_editor_key` had no
         gate at all. Fixed by forwarding the raw key through to
         `Editor::input` instead while `Vim` is active -- `edtui`'s own
         `vim_mode()` table has no entry for this key combination
         either, so it correctly becomes a no-op, the same as any other
         genuinely unbound Vim key, rather than forcing `state.mode`
         into `Visual` outside any of Vim's own bindings.
      2. The dirty-caching perf fix earlier in this file
         (`can_mutate_buffer`, exempting `Left`/`Right`/`Up`/`Down`/
         `Home`/`End`/`PageUp`/`PageDown` from the O(line length)
         `is_dirty` recomputation) gave Vim users none of its own
         benefit: Vim's primary navigation convention is `h`/`j`/`k`/`l`,
         plain `Char` keys, not arrows, so a Vim user navigating the
         exact pathologically-long-line file that fix exists for would
         still pay the full cost on every keypress. Fixed by extending
         the exemption to `h`/`j`/`k`/`l` specifically while
         `keymap_mode == Vim` and the keypress was interpreted under
         Vim's own `Normal`/`Visual` mode (`edtui`'s own
         `vim_keybindings()` binds both keys and arrows to the identical
         `MoveBackward`/`MoveForward`/`MoveUp`/`MoveDown` actions --
         confirmed directly from its source). Deliberately *not*
         extended to Vim's other single-key motions (`w`/`b`/`e`/`0`/
         `$`/...): `w` in particular is reused as the *second* key of
         `dw`/`cw` (delete/change word forward, both genuinely
         mutating), and a per-keypress keycode check with no visibility
         into `edtui`'s own pending multi-key lookup state can't safely
         tell "bare `w` navigating" apart from "`w` completing `dw`" --
         `h`/`j`/`k`/`l` were checked directly against the *entire*
         `vim_keybindings()` table and confirmed to never appear as a
         component of any multi-key sequence, which is exactly why only
         these four are safe.

      Confirmed working correctly (no bug, no fix needed) after direct
      testing: `Editor::select_all` (Ctrl+A) and `Editor::start_search`
      (Ctrl+F) both behave the same in either keymap, since neither
      relies on `Standard`-specific touch-tracking, just plain chained
      `edtui` motions or independent app-level state; closing with `Esc`
      while a genuine Vim `Visual`-mode selection is active (opened with
      real Vim `v`/motion keys, not `Shift`+arrow) correctly cancels the
      selection instead of closing the editor, the same as it already
      did for `Standard`'s own `Shift`-selection; Vim's own `x` (delete)
      and `u` (Undo) both correctly update `is_dirty`, including
      undoing back to exactly the saved content correctly clearing it
      again.
- [ ] Ctrl+V over an active selection doesn't replace it (clears the
      selection, then pastes at the cursor instead) — `edtui`'s real
      "paste over selection" action isn't publicly exported; see
      [[litastum-stack]]
- [ ] **Typing over an active selection doesn't replace it either — same
      class of bug as the Ctrl+V one above, but for plain character
      input.** Reported directly: current behavior is select, delete,
      *then* type; wanted behavior is select, type, and the typed
      character(s) replace the selection in one step (standard
      VSCode/Windows-editor convention this app's own `Standard` keymap
      is otherwise built to match). Not yet investigated for root cause,
      but likely the same underlying gap as Ctrl+V — plain character
      insertion probably goes through `edtui`'s own table unmodified,
      with no equivalent "clear selection first" correction pass the way
      arrow-key/Shift-selection handling already gets in
      `Editor::input`'s post-table correction block.
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
- [x] Syntax highlighting for `.clang-format`/`.clang-tidy` — reported
      rendered as plain text. `Path::extension()` returns `None` for
      both (a leading dot with no further dot, same dotfile gap
      `.gitignore` hit before the name-first lookup tier existed), and
      no grammar anywhere declares the literal file name either — but
      both formats genuinely *are* YAML (clang's own documented config
      syntax), so `EXTENSION_ALIASES` (`editor/syntax/grammars.rs`)
      points them straight at `syntect`'s own bundled YAML grammar,
      same alias mechanism `.rc`/`.rc2` already use for C++, just to an
      exact-match rather than a close-enough grammar.
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
- [x] Bracket-pair matching, Far Manager/VS Code-style -- requested
      directly, along with an explicit constraint: bracket matching must
      stay fully independent of the word-occurrence highlighting above,
      never feeding brackets into it or vice versa. New
      `editor/bracket_match.rs`, a second, entirely separate pass over
      the same `EditorState::highlights` field the word-occurrence
      feature already rides -- not folded into `word_occurrence_highlights`
      itself. In practice the two features could never actually collide
      even without this separation (`word_highlight::is_word_char` only
      ever matches ASCII alphanumerics/`_`, so a bracket is never a
      candidate for it), but keeping bracket matching in its own
      module/function with its own call in `Editor::view` makes that
      guarantee structural rather than incidental -- see
      `bracket_match_highlights`'s own doc comment. Understands `()`,
      `[]`, `{}`, and `<>` (matching only within the same kind -- a `{`
      inside `(...)` is invisible to matching a `(`, the standard rule
      every mainstream bracket-matcher uses) -- `<>` added right after,
      per an explicit follow-up request to add more bracket kinds, like
      `<>`. Documented as a deliberately accepted, known tradeoff
      rather than a gap (`PAIRS`'s own doc comment, plus a dedicated
      test, `angle_brackets_can_mismatch_against_real_comparison_operators`,
      pinning down the exact failure shape): `<`/`>` are also comparison
      operators in every C-like language this editor highlights, and
      this module has no syntax awareness to tell "generic/tag
      delimiter" from "comparison" apart -- a plain same-kind
      nesting-depth scan, its whole strategy, can genuinely mismatch on
      real code containing a bare `<`/`>` comparison. Accepted anyway,
      since balanced angle brackets (`Vec<Option<T>>`, `<div>...</div>`)
      are a far more common real-world shape than a mismatching bare
      comparison, and this is the same tradeoff most mainstream editors
      that support `<>` matching at all already make. A plain forward/backward
      nesting-depth scan from the cursor's position (`find_forward`/
      `find_backward`), same "touching" convention as `word_highlight::
      word_at` for where the cursor counts as being "on" a bracket
      (its own cell, or the cell immediately to its left).
      **Follow-up, requested directly right after landing**: originally
      colored `theme.bg` on `theme.accent` (a deliberately distinct look
      from word highlighting) and only highlighted the bracket's
      *matching partner*, not the one under the cursor -- both reversed.
      Now shares the exact same `Style` as `word_highlight.rs`'s "same
      word as the one under the cursor" feature (`theme.text` on
      `theme.border`, one shared `highlight_style` value in
      `Editor::view` passed to both passes) rather than a visually
      distinct color, and highlights *both* brackets of the pair, not
      just the far one -- `bracket_match_highlights` returns two
      `Highlight`s again (`pos` and `match_pos`), matching real Far/VS
      Code behavior where both sides of a matched pair read as "this
      pair." This is now a deliberate *difference* from `word_highlight`'s
      own choice to exclude the cursor's "home" occurrence from its own
      highlight list -- the two features share a color but not that
      particular behavior, and both choices are correct for their own
      feature (a whole *pair* of brackets reads as one unit; a word
      occurrence is just one of many, so a plainly "at the cursor" one
      for it adds nothing). Skipped for a pathologically long line, same
      reason and same shared `has_pathologically_long_line` check syntax
      highlighting uses just above in `Editor::view` -- an unbounded
      forward/backward scan across such a line would reintroduce the
      exact class of per-frame cost that fix exists to avoid.
      **Second follow-up, reported directly with a screenshot right
      after the above landed**: the near bracket's own highlight still
      wasn't actually visible while the cursor sat exactly on it --
      `edtui` paints the cursor's own cell *after* any `Highlight`
      (`EditorView::render`), so the near bracket's color was there in
      the data but silently overwritten back to plain `base` on screen,
      leaving only the far bracket visibly highlighted. Same fix shape
      already used for an active text selection's own cursor cell
      (`Editor::view`'s `cursor_style` decision): a new
      `bracket_match::cursor_is_on_a_matched_bracket` check (true only
      when the cursor sits *directly* on a bracket that's part of a real
      matched pair, not merely "touching" one from the append position
      one column to its right) now paints the cursor's own cell with
      `highlight_style` too, instead of `hide_cursor()`'s plain `base`,
      whenever it applies -- both brackets of a pair read as highlighted
      together now, confirmed by a real rendering test comparing the
      cursor's own cell color before and after moving onto a bracket
      (`the_bracket_under_the_cursor_is_also_visibly_highlighted`).
- [x] **Third follow-up, reported again with a screenshot -- investigated,
      turned out not to be a highlighting bug**: with the cursor on a
      bracket, sometimes only one side visibly colored. Reproduced
      directly with a real `.json` file and a short `TestBackend`
      viewport matching the reported screenshot's apparent window
      height -- both brackets' *highlight data* was always correct
      (confirmed across cursor-on-open, cursor-past-open via the append
      position, cursor-on-close, and an adjacent same-line pair, all
      with real syntax highlighting engaged), but when the matching
      bracket's row fell outside the currently-scrolled-into-view rows,
      it was never rendered at all -- `edtui`'s own vertical auto-scroll
      only ever keeps the *cursor's* row in view, with no awareness of a
      matched bracket possibly sitting well outside that range. There's
      no cell to paint a color on if it was never drawn to the terminal
      in the first place -- not fixable inside `bracket_match.rs`/
      `Editor::view`'s highlight logic alone, the cause was squarely in
      `edtui`'s own scroll targeting.
      **Fixed**, per explicit follow-up (try widening the viewport to
      fit both brackets), by nudging the viewport ourselves rather than accepting the
      limitation: `Editor::view` now takes the render `area` (threaded
      in from `ui/editor_pane.rs`'s own call site, the only place that
      actually knows the real `Rect` about to be rendered into --
      `Editor` had no other way to learn the current size ahead of
      time), and a new `bracket_match::matched_bracket_row_span` reports
      the matched pair's own inclusive `(top_row, bottom_row)` span when
      it crosses more than one line. If that span fits within the
      approximate content height (`area.height - 2`, for the border --
      no status line to also subtract, `.hide_status_line()` is always
      on), `Editor::view` calls `EditorState::set_viewport_offset` to
      top-align the pair before constructing the `EditorView`. This
      doesn't fight `edtui`'s own auto-scroll-to-cursor
      (`ViewOffset::update_viewport_vertical`, confirmed directly from
      its source): that logic only *overrides* an offset if the cursor
      would fall outside it, and since our chosen offset always includes
      the cursor's own row (it's one of the two rows the span is built
      from), it's left alone. When the pair doesn't fit at all, this
      deliberately does nothing -- `edtui`'s own default (keep the
      cursor's row visible) is the correct fallback, confirmed by a
      dedicated test. Two real rendering tests
      (`a_multi_line_bracket_pair_widens_the_viewport_to_show_both_when_it_fits`,
      `a_bracket_pair_that_does_not_fit_leaves_the_viewport_showing_the_cursor`)
      pin both halves down, including the "fits exactly" boundary case.
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
