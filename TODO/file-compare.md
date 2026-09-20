# File compare / conflict resolver — spec, not yet implemented

Supersedes the placeholder bullet in [next-up.md](next-up.md) ("Diff view
mode + conflict resolver"). Requested directly: a two-panel file
comparer first (two files, side by side, GitHub-diff-style red/green
line highlighting, using litastum's own editor themes rather than a
fixed palette), modeled loosely on real Far Manager's bundled
`merge.exe`; a conflict resolver (3-way merge) is an explicit phase 2,
built on top of phase 1's diff engine and rendering once that's proven,
not designed in full here. This file is the design pass requested
before writing any code — no implementation should start from this
document alone until the open questions in the last section are
answered.

## What real Far Manager's `merge.exe` actually does (for calibration)

`merge.exe` is a *separate bundled executable*, not a Far panel mode —
launched either standalone (`merge.exe file1 file2 [file3]`) or from
Far's own file menu. Two-file mode is a plain side-by-side diff with
synchronized scrolling and jump-to-next/previous-difference navigation;
three-file mode is a real 3-way merge with a bottom "result" pane you
edit directly, plus per-hunk "take left"/"take right"/"take both"
actions. It has its own line-level color scheme (distinct from Far's
panel/editor colors) and its own keyboard scheme (not Far's F-key
convention). litastum's own version should match its *shape*
(side-by-side, sync-scrolled, hunk-navigable, red/green line coloring)
but reuse litastum's own `Theme`/editor infrastructure rather than
inventing a third, independent color system — see "Theming" below.

## Phase 1 scope: two-file compare, read-only

- Two files, side by side, each rendered with this app's own existing
  syntax highlighting (not a diff tool's usual monochrome-plus-color-
  gutter look) — litastum already has real syntax highlighting
  (`editor.rs`/`edtui`'s `SyntaxHighlighter`) and it would be a
  regression to lose it just because the view is now a diff.
- Whole **lines** are colored red (removed, left-only) / green (added,
  right-only) — matching GitHub's own diff convention, per the literal
  request. Word/character-level intra-line highlighting (GitHub's own
  "highlight exactly which part of a changed line changed") is
  explicitly **out of scope for phase 1** — line-level only, revisit
  later if asked.
- Unchanged lines render exactly as the built-in editor already renders
  them (syntax-highlighted, no diff coloring at all).
- Read-only in phase 1 — no editing either pane. (Phase 2's conflict
  resolver is exactly the feature that turns this into something
  editable; keeping phase 1 read-only keeps its own scope small and
  gives phase 2 a proven rendering base to build the editable/resolve
  actions on top of, rather than building both at once.)
- Navigation: jump to next/previous diff hunk, scroll both panes in
  sync. Exact key choices are an implementation-time detail, not
  pinned down here, except where a real conflict already exists with
  something else this app binds (see "Keybinding" below).

## Entry point / keybinding

Checked every F-key, `Ctrl+`, `Alt+`, and `Shift+F` combination already
claimed in this codebase (`.claude/rules/litastum-command-line.md`,
`explorer/keymap.rs::resolve`, `ui/function_keys.rs`) before proposing
anything:

- Bare F-keys `F1`–`F10` are all either bound (`F2` user menu, `F3`
  preview, `F4` edit, `F5` copy, `F6` move, `F8` delete, `F9` menu,
  `F10` quit) or shown as a live but *unwired* label (`F1` "Help", `F7`
  "Folder"/mkdir) — using either unwired one for compare would need the
  F-key bar's label relabeled too, and would collide with a real Far
  convention users of this app may already expect (`F7` = make
  directory).
- `Shift+F6` is claimed (rename). `Alt+F1`/`Alt+F2` (drive switch),
  `Alt+F7` (Find file), `Alt+F8` (command history) are claimed.
  `Ctrl+O`/`Ctrl+P`/`Ctrl+U` are claimed (panels/shell/swap).
- **No `Ctrl+F<n>` combination is bound to anything, anywhere, in this
  codebase.** Real Far Manager itself has no single fixed shortcut for
  file compare either (it's a file-menu action) — this isn't
  reinventing an existing well-known binding, just picking a free one.

**Recommendation**: `Ctrl+F5` opens Compare on the active panel's
selected file plus a picker for the second (matching Far's own F5/F6
=copy/move convention of "5 and 6 are the file-operations neighbors" —
`Ctrl+F5` reads as "F5, but compare instead of copy"). Reachable a
second way through **F9 → Commands → Compare files**, mirroring how
Find file is both a menu item *and* has a direct shortcut layered on
top (`Alt+F7`) — the menu route needs no shortcut decision to ship
first, and a shortcut can follow the same way Alt+F7 followed Find
file's own menu entry. This is a recommendation, not a final decision —
see "Open questions" below.

## Rendering approach — the one architecturally significant decision

Two real options, both technically workable, with different long-term
cost:

**(a) Two `edtui::EditorView`s, one per pane**, reusing the exact
mechanism the built-in editor already uses for word-occurrence and
bracket-pair highlighting: `EditorState::highlights: Vec<Highlight>`
(see `editor/word_highlight.rs`). A whole-line `Highlight` from
`Index2::new(row, 0)` to the line's own last column, styled
`Style::default().fg(...).bg(diff_removed_bg)`/`.bg(diff_added_bg)`,
gets syntax-highlighted text *and* a diff-colored background painted
together, since `edtui` layers highlights over its own base syntax
styling already (confirmed in `editor/mod.rs`'s own `view` doc
comments — same layering `word_occurrence_highlights` already relies
on). Gets real syntax highlighting for free; costs having to suppress
`edtui`'s own cursor/editing semantics for a genuinely read-only view
(`hide_cursor()`, never forwarding key events into
`EditorEventHandler`) — a shape this codebase hasn't proven yet
(every existing `EditorView` use is a real, editable buffer).

**(b) A hand-rolled renderer** (`ratatui::widgets::Paragraph`/manual
`Span` construction per visible line), building each line's spans from
scratch: this app's own existing syntax highlighting machinery would
need to be called directly (bypassing `EditorView`) to still get
colored code, then diff backgrounds layered on top of *that* per-span
styling by hand. More code, but no fighting an editing-widget's own
assumptions for a view that was never going to be edited in phase 1
anyway — and if phase 2's conflict resolver *does* need real editing
in the "result" pane, that one pane specifically can become a real
`Editor`/`EditorView` at that point, while the two read-only
comparison panes stay simple.

**No default picked here** — this is exactly the kind of call worth
confirming before writing code, since it shapes how much of phase 1's
own code phase 2 can reuse. See "Open questions."

## Diff algorithm

No diff/text-comparison crate exists in this project yet
(`Cargo.toml`). Following the same justification shape already used for
adding `ignore` (`find_file/search/walk.rs`'s own doc comment: name a
well-known consumer of the same crate, pick something pure-Rust with no
C-toolchain requirement per `.claude/rules/litastum-stack.md`): the
`similar` crate (pure Rust, no C dependency, an established choice for
exactly this line-level-diff job in the Rust ecosystem) is the natural
pick — `similar::TextDiff::from_lines(left, right)` produces exactly
the per-line "equal/delete/insert" hunk classification needed to build
either rendering approach's own removed/added `Highlight`/`Span` list
directly, with no extra translation layer.

## Theming

`Theme` (`theming/theme.rs`) has no field aimed at a *whole-line
background fill* today — `danger`/`success` are used everywhere else in
this codebase as small foreground accents (a file-type color, a
label), never a full-saturation background wash across an entire line
of text, and using them verbatim as `.bg()` would likely read far
harsher than GitHub's own diff colors (which are deliberately dim/
desaturated, not raw brand red/green, against a dark background).

Two options, not decided here:
- **(a)** Compute a dimmed `.bg()` from `theme.danger`/`theme.success`
  at render time, blended toward `theme.bg` — the same *kind* of
  precomputation `current_row_bg` already documents (`accent` blended
  ~15% over `bg`), but there's no existing reusable "blend two colors"
  helper in this codebase yet (`current_row_bg` for a *custom* scheme
  is read straight from the scheme's own `selectionBackground`, per
  `theming/scheme.rs`, not computed by blending at runtime) — this
  would be new, shared utility code, not a call to something that
  already exists.
- **(b)** Two new dedicated `Theme` fields (e.g. `diff_removed_bg`/
  `diff_added_bg`), following the exact precedent
  `command_line_prefix`/`selection_text` already set: a litastum-
  specific concept with no natural Windows Terminal JSON slot to derive
  from, `Option<Color>` with a sensible computed fallback (probably (a)
  above, used as the fallback *inside* this option rather than as an
  alternative to it) for every scheme that doesn't set it explicitly,
  plus a new optional field in `ColorScheme`
  (`themes/*.json`'s own extension mechanism, `#[serde(default)]`,
  same shape as `commandLinePrefix`/`selectionForeground`).

Foreground text color over a diff-colored background needs the same
consideration `selection_text` already exists for (a bright diff
background could make ordinary text/syntax-highlighted text hard to
read) — likely reusing that exact field/fallback rule rather than
inventing a third "text-over-a-colored-background" concept.

## Data model sketch (not final)

- `CompareState { left: ComparePane, right: ComparePane, hunks: Vec<DiffHunk>, current_hunk: usize }`
- `ComparePane { path: PathBuf, lines: Vec<String>, scroll_offset: usize, syntax_highlighter: ... }` (or two real `Editor`/`EditorState`-shaped buffers if rendering approach (a) is chosen)
- `DiffHunk { kind: Added | Removed | Unchanged, left_range: Range<usize>, right_range: Range<usize> }` — direct output shape from `similar::TextDiff`'s own grouped-ops API
- New `Mode::CompareFiles(CompareState)` variant (`app.rs`), a new
  top-level module (`compare.rs` → `compare/` once it grows, mirroring
  `editor.rs`/`editor/`'s own split — this is a new top-level concern,
  not an `explorer::` sub-feature: it doesn't browse files, and it
  isn't editing one either) with its own `handle_compare_key`, and a
  new `ui/compare.rs::draw_compare` — following the "full-screen
  takeover, `return` before the ordinary 2-panel layout runs"
  dispatch shape `Mode::Editing` already uses in `ui/mod.rs`, not the
  panel-slot-replacement shape `Mode::ImagePreview` uses (a dedicated
  compare view wants two full-width panes of its own, not one browser
  panel's worth of space).

## Explicitly out of scope for phase 1

- 3-way merge / conflict resolution UI (phase 2 — this document
  doesn't design it beyond acknowledging it comes next; see
  [next-up.md](next-up.md)'s own still-open questions, e.g. how/whether
  it hooks into git conflict markers, cross-linked in
  [git-integration.md](git-integration.md)).
- Word/character-level intra-line diff highlighting (GitHub's "which
  part of the line changed" — line-level only for now).
- Directory/folder compare (real Far Manager's `Ctrl+F10`-class
  feature is a separate thing from `merge.exe`) — this spec is file-
  vs-file only, per the literal request (explicitly scoped to two
  files for now, not a whole tree).
- Any editing in phase 1's own two panes (read-only, see above).
- A dedicated compare-history/persistence feature (unlike Find file's
  own two histories, `find_file/history.rs`) — not asked for, no
  obvious "recall a past comparison" use case yet.

## Decisions (confirmed directly, superseding "Open questions" below)

1. **Rendering approach**: **(a)** — two suppressed-input `EditorView`s
   reusing `Highlight`, gaining syntax highlighting for free. Phase 2's
   editable "result" pane can become a real, input-forwarding `Editor`
   later; the two comparison panes stay read-only.
2. **Keybinding**: **`Alt+F5`** — currently cosmetic-only (`ALT_LABELS`
   shows `"Copy"`, same as bare `F5`; `explorer::keymap::resolve` keys
   off `KeyCode` alone with no modifier awareness, so `Alt+F5` silently
   falls through to `Command::CopySelected` today with nothing actually
   intercepting the modifier). Needs a new raw-modifier special case in
   `command_line/browsing/mod.rs::handle_browsing_key`, ahead of the
   generic `keymap::resolve` dispatch — the exact same shape
   `Alt+F1`/`Alt+F2`/`Alt+F7`/`Alt+F8` already use there (each checks
   `key.code == KeyCode::F(n) && key.modifiers.contains(KeyModifiers::ALT)`
   before anything else). `ui/function_keys.rs::ALT_LABELS`'s own `F5`
   entry needs relabeling too (currently `"Copy"`, cosmetically implying
   no rebinding — same trap already documented for the four keys above
   before *they* were wired).
3. **How the second file is chosen**: **both panels** — `Alt+F5` opens
   Compare directly on the active panel's selected file (left pane) and
   the *other* (inactive) panel's own currently-selected file (right
   pane), Far Manager's own real two-panel convention, no picker popup
   in phase 1.
4. **Theming**: deferred to implementation time which of (a)/(b) from
   "Theming" above is used — not blocking the rest of the design.

## New requirement: an F9 submenu for line-ending display

Requested directly, alongside the decisions above: Compare's own `F9`
opens a menu (mirroring the built-in editor's own `F9` → Keybindings
two-level structure — `EditorMenu` → `EditorKeymapMenu`,
`editor/menu.rs`/`editor/keymap_menu.rs`) with a "Line endings" item,
itself a picker between **Hidden** (default) and **Shown**. When
**Shown**, every rendered line gets a small trailing marker indicating
whether *that specific line* actually ends in `CRLF` or `LF` —
detected per line from the real file bytes at load time, not a global
per-file assumption — the classic real use for this in a file
comparer: two files can be textually identical yet still "different"
purely because of a line-ending mismatch (one edited on Windows, the
other on Unix), which plain syntax-highlighted text would never
surface on its own. Persisted the same way `PopupStyle`/
`EditorKeymapMode` already are (`theming::config`, a new
`compare_line_ending_display: Option<...>` field in `Config`,
`load_active_.../set_...` functions following that exact pattern).

## Open questions (still genuinely open, non-blocking for a first pass)

- Whether a file with *no* line ending on its final line (no trailing
  newline at all) needs its own third visual state, or is simply
  unmarked — **resolved below, landed as "simply unmarked."**

## Landed (phase 1)

Implemented as designed above, no scope changes from the four
"Decisions" beyond resolving the two items still open at design time:

- **Theming**: option **(b)** — two dedicated `Theme` fields,
  `diff_removed_bg`/`diff_added_bg` (`theming/theme.rs`), rather than a
  runtime blend computed ad hoc at render time. Backed by a new, real,
  tested `blend_over_bg(fg, bg, alpha)` helper (`danger`/`success`
  blended 20% over `bg`) — `Theme::dark()`'s own literal values are the
  hand-computed result of that same blend, and a custom
  `ColorScheme::to_theme()` (`theming/scheme.rs`) derives both fields
  from the scheme's own `danger`/`success`/`bg` the same way, so every
  theme (built-in or a user's own Windows Terminal JSON) gets sensible
  diff colors with no per-scheme opt-in required. No new `ColorScheme`
  JSON field was needed — unlike `commandLinePrefix`/`selectionForeground`,
  there was no case for letting a scheme author override this directly.
- **Line-ending marker**: `" [CRLF]"`/`" [LF]"`, plain text appended to
  the line itself (not a separately-styled span) — a changed line's
  whole-row `Highlight` already recolors the entire row uniformly
  (`fg(theme.text)`), so the marker inherits that same color rather
  than getting its own dimmed style; an unchanged row's marker renders
  in whatever the base editor theme/syntax highlighter already uses for
  plain text at that position. A file's own final line with no trailing
  newline gets no marker at all (`line_ending::detect` returns `None`
  for it) — the "simply unmarked" option from the list above, not a
  third visual state.
- **Rendering** matches decision 1 exactly: `ui/compare.rs::draw_pane`
  builds two `edtui::EditorView`s per frame (`hide_cursor()`, no key
  events ever forwarded in), each fed a fresh `EditorState` built
  straight from `ComparePane::lines` (plus the optional marker) —
  deliberately *not* a persisted `EditorState` the way the real editor
  keeps one; see `ComparePane`'s own doc comment
  (`src/compare/state.rs`) for why nothing but the scroll row needed to
  survive between frames.
- **Syntax highlighting inside a diffed line is not wired up** — a
  known, documented limitation, not an oversight: `edtui::Highlight`'s
  style fully replaces whatever span it lands on (confirmed against the
  same behavior `editor/word_highlight.rs` already documents for
  word-occurrence highlighting), so a changed line renders as flat
  themed text, same tradeoff an active text selection already accepts
  elsewhere in this app. `resolve_syntax_highlighter`
  (`src/editor/syntax/mod.rs`) also stayed `pub(super)`-scoped to
  `editor::` rather than being widened for this feature — revisit both
  together if word/character-level diff highlighting (still out of
  scope, see above) is ever picked up.
- **Keybinding, second-file selection, F9 submenu**: landed exactly as
  decisions 2/3 and the "New requirement" section above describe — no
  deviation.

Not started: phase 2 (3-way conflict resolver) — deliberately, per this
document's own scope note in the intro; revisit explicitly with the
next request rather than continuing straight on from phase 1.

## Landed (phase 1.5): both panes made fully editable

Requested directly, right after phase 1 shipped: both panes should be
editable, the way real Far `merge.exe` lets you edit either side of a
two-file compare directly, not just look at it. Confirmed with the user
up front on the one question that actually mattered here: **keep exact
row alignment between the two panes while editing** (the harder of two
options — the simpler one, letting the panes' own row counts drift
independently like VS Code's diff editor does once you start typing,
was the fallback if this turned out impractical).

This superseded several of phase 1's own "Landed" claims above, which
now describe a design this section replaces:

- **`ComparePane` (`compare/state.rs`) no longer exists.**
  `CompareState` now holds two ordinary, independent, fully live
  `editor::Editor` sessions (`left`/`right`) plus `focus: Side` (which
  one currently owns the real terminal cursor and receives typed
  input, toggled by `Tab`) — real cursor movement, undo, syntax
  highlighting, and `Ctrl+S` save, identical to `F4` editing, because
  it *is* `F4` editing's own `Editor` type, reused rather than
  reimplemented.
- **The diff is recomputed fresh every single frame**
  (`ui/compare.rs::draw_compare`, `diff::compute`), straight from both
  panes' *live* text (`Editor::text`, a new accessor) — not a snapshot
  taken once at `open`. Red/green highlighting tracks live edits on
  either side, not just what was on disk when Compare was opened.
- **Neither pane's real buffer ever has synthetic filler lines
  injected into it** (a hard requirement once panes are genuinely
  editable and saved back to disk — phase 1's own filler-padded display
  buffer, safe only because it was thrown away every frame and never
  written anywhere, would otherwise get saved as literal garbage lines
  the moment `Ctrl+S` ran). `diff::compute`'s row-aligned `Empty`
  padding rows still exist, but purely as a classification device now
  — `DiffLines` dropped its own `lines: Vec<String>` field entirely,
  keeping just `kinds`/`source_index`.
- **Exact row alignment is kept anyway**, without touching either
  buffer, through a different mechanism: only the *focused* pane
  scrolls under its own steam (`Editor::view`'s completely unmodified
  cursor-follow behavior); the *other* pane's viewport is forced, every
  frame, to whatever real row corresponds to the focused pane's current
  top row (`diff::map_real_row`, `Editor::set_viewport_top_row` — a new
  method with the *exact* same "also overwrite `state.cursor.row`, not
  just the viewport offset" fix phase 1's own real scroll bug needed,
  see that method's own doc comment for why). A pane's true cursor
  position is cached (`CompareState::left_saved_cursor`/
  `right_saved_cursor`) the instant it loses focus and restored the
  moment it regains it, since its own `Editor::cursor` gets hijacked for
  viewport-sync purposes the whole time it's unfocused.
- **Highlights are layered onto a real, editable `Editor` via a new,
  general hook** (`Editor::extra_highlights`/`set_extra_highlights`,
  merged into `view()`'s own computed `state.highlights` on top of
  whatever word-occurrence/bracket-pair highlighting it already does)
  rather than building a whole separate `EditorState` the way the
  read-only version did — this is the one change to `editor::Editor`
  itself this phase needed, and it's inert (empty, no-op) for every
  other caller (plain `F4` editing never sets it).
- **The line-ending marker's own rendering had to change shape
  entirely.** Baking `" [CRLF]"`/`" [LF]"` directly into the text fed to
  `Lines::from` (phase 1's approach) is no longer safe once that text
  *is* the real, saved buffer — the marker would get written into the
  file itself the next time `Ctrl+S` ran. Now drawn as a separate
  right-aligned overlay `Paragraph` on top of the already-rendered
  `EditorView` (`ui/compare.rs::draw_line_ending_overlay`), positioned
  by approximating `Editor::view`'s own bordered content rect (no
  direct hook into its real internal layout exists, but line numbers
  only ever affect the *left* edge, so the approximation only needs to
  be right along the right edge, which it is).
- **A real, separate bug this surfaced**: detecting line endings from
  `Editor::text()` at render time (the first attempt) can *never* find
  a `CRLF` at all — `edtui::Lines::from` normalizes `\r\n` to `\n` on
  load (`str::lines()`, confirmed directly from source, strips both
  uniformly), so the distinction is destroyed before it ever reaches
  application code, not merely hard to reach. Fixed by having
  `CompareState::open` read each file's own raw bytes once, up front,
  purely to capture `line_ending::detect`'s result before `Editor::open`
  ever touches the same file a second time and loses it — a fixed
  on-open snapshot (`CompareState::line_endings`), not something
  re-derived from the live buffer. **Known, accepted limitation**: since
  this snapshot is indexed by real line number and edits that insert or
  remove lines shift every later line's index, the marker can drift out
  of sync with which physical line is which the more a file is edited
  after opening — still meaningfully useful for the actual common case
  (spotting a mixed-line-ending file before or shortly after starting to
  edit it) than not showing it at all.
- **`Esc` with unsaved changes in either pane** now asks first
  (`Mode::CompareConfirmDiscard`, reusing the built-in editor's own
  "Unsaved changes" popup and `Y`/`N`/`Esc` resolver verbatim —
  `editor::resolve_confirm_discard`/`ConfirmDiscardCommand`, exported
  from `editor.rs` for exactly this reuse) rather than discarding
  silently, matching `F4` editing's own `Mode::ConfirmDiscard`
  convention. `Ctrl+S` saves whichever pane currently has focus — there
  is no "save both at once" gesture, the same one-file-at-a-time
  convention `F4` editing already has.
- **Rebound bindings inside Compare, since `Tab` could no longer mean
  "jump to next diff hunk"**: `Tab` now means "switch pane focus"
  (this app's own convention for `Tab` everywhere else), so hunk
  navigation moved to `Ctrl+Down`/`Ctrl+Up`.
- **Visual focus indication is a known gap, not fixed here**: which
  pane is "hot" is only conveyed by the real terminal cursor blinking
  there (and by where typed characters land) — `Editor::view()` always
  paints its own border in `theme.accent` regardless of focus, and
  changing that would mean threading a border-color override through a
  method every other `Editor` caller (`F4` editing included) uses
  unconditionally. Left alone for this pass; revisit if it turns out to
  matter in practice once tried for real.
