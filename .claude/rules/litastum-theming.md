# litastum: theming

## Format: Windows Terminal color scheme JSON, reused as-is

Theme files are exactly the JSON shape Windows Terminal itself uses for
`colorSchemes` — `name`, `black`..`white` + `bright*` (16 ANSI slots),
`background`, `foreground`, `selectionBackground`, `cursorColor`, all
`"#rrggbb"`. No conversion tool: a downloaded file drops straight into
the themes directory. Parsing lives in `scheme.rs::ColorScheme`.

**Source for ready-made schemes**: <https://windowsterminalthemes.dev/>
— pick one there, download its JSON, and drop it into `themes/` (either
next to the binary/repo, per the cwd fallback below, or the config
dir's own `themes/` subdirectory). This is where the schemes bundled in
this repo (`themes/apple-system-colors.json`, `themes/alien-blood.json`,
`themes/dracula.json`) came from.

`themes/github-dark.json` came from `mbadolato/iTerm2-Color-Schemes`'
Windows Terminal mirror instead (same JSON shape, a different but
equally common distribution point for it), with `background`/
`foreground`/`selectionBackground`/`cursorColor` hand-adjusted toward
`#0d1117`/`#e6edf3` (the same "color 2" values `Theme::dark()` already
used) — **this turned out not to actually match the user's own real VS
Code theme**, reported directly once compared side by side: VS Code's
GitHub theme extension ships several genuinely different dark palettes
(`GitHub Dark`, `GitHub Dark Default`, `GitHub Dark Dimmed`, `GitHub
Dark High Contrast`, ...), and the one most people actually have
selected — the extension's own default — is `GitHub Dark Default`, not
the older `GitHub Dark` this file approximates.

`themes/github-dark-default.json` — the actual **default**, see below —
fixes this by going straight to the source of truth instead of a
third-party mirror: every value (`background`, `foreground`, all 16
ANSI colors) pulled directly from `@primer/primitives`' own published
color tokens (`dist/docs/functional/themes/dark.json` — GitHub's design
system package, which the VS Code theme itself is generated from),
confirmed to genuinely differ from `github-dark.json`'s values (e.g.
`foreground` is `#c9d1d9` here, not `#e6edf3`; every ANSI color is
shifted). `selectionBackground` still computed the same way VS Code's
own theme source does it — `fgColor-accent` (`#58a6ff`) at 20% alpha
over `background`. `github-dark.json` is kept around, unchanged, as a
separate, still-selectable theme for the classic variant — see
`config.rs::default_theme`'s own doc comment for why it's no longer
treated as *the* default.

## Interface theme and editor theme are independent, like in Far Manager

Far Manager (this project's namesake) keeps its interface color scheme
— panels, menus, dialogs, status bar — and its editor's syntax
highlighting as two entirely separate systems with independent
selection; neither is derived from the other. We follow the same
split: `config.json` has two keys, `interface_theme` and
`editor_theme`, each naming a theme file independently. Either can be
set without the other, both can name the same file if a user *does*
want one scheme driving everything, or both can be omitted.

An earlier version of this had one shared `theme` key driving both
halves at once — changed after using it revealed that's not actually
what "customize the interface" means once you can also customize the
editor separately; forcing them to share a file was the bug, not a
feature.

## Where files live and how the active one is chosen

- `<OS config dir>/litastum/config.json` — `{ "interface_theme":
  "<name>", "editor_theme": "<name>" }`, where each `<name>` is a
  filename stem (no `.json`) in the `themes/` subdirectory next to it.
  Missing config dir/file, a missing key, or any parse failure at any
  step falls back to that *half's* built-in default — `config.rs::default_theme`,
  the bundled `themes/github-dark-default.json` scheme, applied to
  **both** halves (interface and editor syntax highlighting) when
  nothing usable is configured for that half — a broken `editor_theme`
  doesn't take the interface theme down with it, and vice versa.
  `default_theme` itself falls back one level further, to the hardcoded
  `Theme::dark()`/`syntax::SYNTAX_THEME` ("dracula"), only if
  `github-dark-default.json` is itself somehow missing or fails to parse
  — the same "never blocks
  startup" guarantee, just one level deeper than before this file
  existed (the built-in default used to *be* the hardcoded value
  directly; now loading a real file is one more thing that can, in
  principle, fail). Logged (`warn!` for "something was there but
  broken", `debug!` for "nothing configured" — see
  `.claude/rules/logging.md`'s level semantics). Never blocks startup or
  panics — `config.rs::load_active_theme` is the one entry point, called
  once in `main()`.
- Resolved via the `directories` crate (a dependency since the initial
  scaffold specifically for this, per [[litastum-stack]], but this is
  its first actual use): `ProjectDirs::from("", "", "litastum")`. On
  Windows that's `%APPDATA%\litastum\`; Linux `~/.config/litastum/`;
  macOS `~/Library/Application Support/litastum/` — standard per-OS
  convention, not hardcoded by us.
- **Theme files are also searched for in `./themes/`, relative to the
  current working directory**, as a fallback after the config dir
  (`config.rs::theme_search_dirs`) — `config.json` itself is *not*
  searched this way, only the theme JSON files it names. Added after
  the config-dir-only version shipped and a theme sitting right in the
  file panel (the repo's own bundled `themes/apple-system-colors.json`,
  cwd = repo root under `cargo run`) didn't show up in the F9 picker —
  a real usability gap, not just a docs gap, since copying a file
  before you can even preview it is backwards. `cargo test`'s cwd is
  also the package root, so this same path has real regression
  coverage (`config.rs`'s `*_via_cwd_fallback` tests), not just a
  scratch-dir simulation.
- `themes/apple-system-colors.json` is checked into the repo root (the
  scheme the user pasted while asking for this feature) as a
  known-working example, found automatically via the cwd fallback
  above when run from the repo root — and it's also the fixture the
  tests in `scheme.rs` parse.
- `themes/github-dark-default.json` is the actual out-of-the-box
  **default** — requested directly ("хочу добавить и сделать её
  дефолтной"), not just another example; retargeted from
  `github-dark.json` once that turned out not to match the user's own
  real VS Code theme (see the source-for-ready-made-schemes section
  above). `config.rs::default_theme` looks it up by name
  (`"github-dark-default"`, the filename stem) through the exact same
  `find_scheme`/cwd-fallback path as every other theme, so it's found
  automatically under `cargo run` from the repo root the same way
  `apple-system-colors.json` is — a real, from-the-repo default, not a
  separately-embedded/hardcoded copy of its colors.
- **F9 → Options → Color schemes** opens `theme_menu.rs` (`menu.rs`
  is the F9 top menu itself — not Far Manager's real top-menu bar,
  just enough structure to reach this and `Options`' sibling `Save
  setup`/`Commands`' `Find file`/`History` — see `TODO/f9-menu.md`): lists
  whatever
  `list_theme_names()` finds in `themes/` at that moment, `Enter`
  applies the highlighted one as *both*
  `interface_theme` and `editor_theme` (the common case), `I`/`E` apply
  to just one side (keeping them independent, per the section above).
  Applies live — no restart — and persists the choice back to
  `config.json` (`config.rs::set_interface_theme`/`set_editor_theme`);
  a failed write is logged but doesn't block the live preview from
  applying. Editing `config.json` by hand still works exactly as
  before; the menu is just a faster path to the same file.

## Mapping conventions (decided unilaterally, easy to revisit)

Windows Terminal schemes have no "accent"/"border"/"danger" concept —
they're just 16 general-purpose ANSI slots plus 4 UI slots. The mapping
onto our own `Theme` (`scheme.rs::ColorScheme::to_theme`):

| Our `Theme` field | WT source                                          |
|--------------------|----------------------------------------------------|
| `bg`               | `background`                                        |
| `text`             | `foreground`                                        |
| `text_dim`, `border`| `brightBlack` (both — no separate WT slot for either) |
| `current_row_bg`   | `selectionBackground`                               |
| `danger`           | `red`                                               |
| `warning`          | `yellow`                                            |
| `success`          | `green`                                             |
| `accent`           | `cursorColor`, unless it equals `background` or `foreground` (falls back to `blue`) |
| `command_line_prefix` | `commandLinePrefix` (litastum-specific extension, see below), falls back to `accent` if absent |

`success`/`warning` were in the original "color 2" plan
(`.claude/rules/litastum-ui-theme.md`) but only `danger` actually
landed in `Theme` initially — added properly once file-type coloring
below needed them.

### `commandLinePrefix`: one litastum-specific, non-standard field

Every other `Theme` field is derived from one of the 16 standard ANSI
slots or the 4 standard WT UI slots — a real Windows Terminal scheme
JSON always has enough to fill in the whole mapping table above. The
command line's own `"{cwd}> "` prefix broke that pattern: reproducing
real Far Manager's own color scheme (`far-lts-alien.json`, extracted
from a real Far install's `colors.db` + console palette — see its own
`CommandLine.Prefix` color group) needed a bold terracotta/orange
(`#d7875f`) that doesn't correspond to any of the 16 named ANSI colors,
so there's no principled way to derive it the way `accent`/`danger`/
etc. are derived.

`ColorScheme::command_line_prefix: Option<String>` is an optional
`#[serde(default)]` field for exactly this — an extra, non-standard
`"commandLinePrefix": "#rrggbb"` key that a plain Windows Terminal
scheme (from windowsterminalthemes.dev or exported by the terminal
itself) will never have, and doesn't need: `to_theme()` falls back to
the same `accent` color when it's absent, which is what every scheme
predating this field already effectively got (the command line prefix
used `theme.accent` directly before `Theme::command_line_prefix`
existed). Rendered bold (`ui/mod.rs::draw_command_line`), matching real
Far's own `[x] Bold` style flag on this color group.

### `selectionForeground`: another litastum-specific field, optional with a different default

Same shape as `commandLinePrefix` above — `ColorScheme::selection_foreground:
Option<String>`, `#[serde(default)]`, absent from every scheme sourced
from windowsterminalthemes.dev — but the fallback is deliberately
different: `None` means *leave the text color alone*, not "substitute a
fixed color." Forcing a uniform text color over `current_row_bg`
(the panel's own cursor row, the editor's own text selection) would
undo the file-type-coloring convention below for every scheme that
never asked for this — `theme.text`/a file's own type color already
reads fine over every built-in scheme's own muted `selectionBackground`.

Added directly for `themes/molocai.json`: its `selectionBackground` was
deliberately changed from the real Molokai scheme's own pale-blue
`#b5d5ff` to its own bright ANSI `green` (`#98e123`, a bolder highlight
the user wanted after browsing the theme's demo on windowsterminalthemes.dev
— that green is actually the theme's `green` swatch used for a Jest
diff highlight in the site's own demo content, not literally
`selectionBackground`, but was liked well enough to become the real
selection color here) — black text (`"selectionForeground": "#000000"`)
keeps it readable ("внутри чёрный текст"). `ubuntu.json`/`alien-blood.json`/
`dracula.json` got the same treatment shortly after, each picking a
vivid color already in its own 16-slot palette (Ubuntu's `brightPurple`,
AlienBlood's `brightGreen`, Dracula's own iconic `green`) rather than
the source's own generic, uncustomized `selectionBackground` (several
of these community schemes never bothered setting one, and all share
the exact same pale-blue `#b5d5ff` as a result — accurate to the real
scheme, just visually generic and often clashing with that scheme's own
identity).

**This surfaced a second gap once the file panel's own selected row
started using black-on-green correctly**: every *other* popup's own
selected-row/text-selection styling (`ui/menu.rs`, `theme_menu.rs`'s
sibling pickers, `find_file.rs`, `command_line.rs`'s history popups,
the Copy/Move destination field, ...) had the identical
`Style::default().fg(theme.text).bg(theme.current_row_bg)` literal
repeated independently in each file, none of them aware of
`Theme::selection_text` — reported directly from a screenshot of the
F9 menu still showing light text on the new green background.
`ui/popup.rs::{selected_row_style, selected_text_style}` now centralize
this (bold for a list row, plain for an in-place text selection); every
popup that used to write that `Style` out longhand calls one of these
instead, so a future theme's `selectionForeground` reaches every popup
at once rather than needing to be threaded through each one by hand
again.

**One real spot still missed that pass**: `ui/markdown_preview.rs::render_line`'s
own highlighted-line painting (the row `MarkdownPreviewState::sync_to_editor_cursor`
matches to the built-in editor's cursor, in the linked embedded
preview) — reported directly from a screenshot of the split editor+
preview view, green background still showing light text on the
*preview* side while the *editor* side (same line, same file) already
showed black correctly. Didn't go through `ui/popup.rs`'s two helpers
at all: it needs to layer `current_row_bg` *on top of* each span's own
`span_style` kind-based color (heading/bold/link/... — a markdown
line isn't uniformly `theme.text` the way a popup's plain list row is),
which neither helper's fixed starting color fits. Fixed in place
instead — the same `if let Some(selection_text) = theme.selection_text`
override, applied only inside the `highlighted` branch, right after the
background is layered on.

## File-type coloring (`panel.rs::HighlightRole`)

Far Manager-style: entries are colored by a coarse category, not left
uniform — reverses an earlier "no file-type color dots, deliberate
minimalism" decision (see [[litastum-ui-theme]]) per an explicit later
request. Deliberately small — a handful of common extensions, not
Far's own regex-based `highlighting.hgh` rule system:

| `HighlightRole`   | Matches                                              | Color             |
|--------------------|-------------------------------------------------------|--------------------|
| `Parent`           | `..`                                                   | `text_dim`         |
| `Directory`        | an ordinary directory                                  | `text` (no distinction) |
| `VcsDirectory`     | `.git .svn .hg .bzr`                                   | `accent`           |
| `Archive`          | `.zip .7z .rar .tar .gz .bz2 .xz`                      | `warning`          |
| `Executable`       | `.exe .bat .cmd .sh .ps1 .py .js .ts .rb .pl`          | `success`          |
| `Other`            | everything else                                        | `text`             |

`Directory` is *not* colored — checked against a real Far Manager
screenshot while building this, and ordinary directories (`.cargo`,
`src`, `target`, ...) render there in plain text same as files; the
first version of this colored every directory, which the reference
didn't actually support. The one directory case Far's screenshot did
color distinctly was `.git` — generalized to `VcsDirectory` (the common
VCS metadata dir names) rather than hardcoding `.git` alone. The color
itself still comes from whichever scheme is active (`theme.accent`),
not copied from Far's own palette — only the *rule* (VCS dirs get a
distinct color) is what's borrowed.

The exact category→color choices otherwise are a first pass, not
researched against Far's actual default `highlighting.hgh` (which
varies by config anyway) — easy to retune in
`panel.rs::Entry::highlight_role` and the match in
`ui.rs::build_list_item` if they look wrong in practice.

## Syntax highlighting: base16-style derivation

When `editor_theme` names a scheme, the editor's `syntect` theme
(`scheme.rs::ColorScheme::to_syntax_theme`) is derived from that
scheme's 16 colors, base16-style, rather than the unrelated hardcoded
named theme (`editor.rs::SYNTAX_THEME = "dracula"`, which is what's
still used with no `editor_theme` configured — non-breaking default,
independent of whatever `interface_theme` is set to). Windows
Terminal's 8 base colors line up closely with
base16's keyword/string/comment/etc. roles — this is base16's own
original design, not a stretch invented here:

- `keyword`, `storage` → `purple`
- `string` → `green`
- `comment` → `brightBlack`
- `constant.numeric`, `constant.language` → `yellow`
- `entity.name.function`, `support.function` → `blue`
- `entity.name.type`, `entity.name.class`, `support.type` → `cyan`
- `variable.parameter`, `entity.name.tag` → `red`
- `markup.heading` (bold) → `cyan`, `markup.bold` (bold) → `yellow`,
  `markup.italic`/`markup.quote` (italic) → `purple`/`brightBlack`,
  `markup.list`/list-item punctuation → `red`, links → `blue`, code
  spans/blocks → `green`, heading/bold/italic/link punctuation markers
  → `brightBlack` — added after a report that `.md` files "have no
  highlighting" under a *custom* `editor_theme`: the highlighter was
  genuinely running (`syntect`'s bundled default set does include
  Markdown), but every `markup.*` scope it emitted fell through to
  plain `foreground` with nothing here naming it, since the original
  scope list above was code-only. The built-in `dracula` fallback
  theme already had real `markup.*` rules of its own, which is why
  this only showed up once a custom scheme was applied
- `markup.inserted` → `green`, `markup.deleted` → `red`,
  `markup.changed` → `yellow`, `meta.diff.header`/`meta.header` (bold)
  → `cyan`, `meta.diff.range`/`punctuation.definition.range` → `blue`,
  `meta.separator.diff` → `brightBlack` — same exact gap as the
  Markdown one above, hit again for `.diff`/`.patch`: `syntect`'s own
  bundled default set already resolves them to a real "Diff" grammar
  (no `BUNDLED_GRAMMARS` entry needed — confirmed directly), but this
  theme had nothing naming its `markup.inserted.diff`/
  `markup.deleted.diff`/`markup.changed.diff`/`meta.diff.*` scopes
  either. Reported as "works with an older build that has no
  `editor_theme` configured, not with one that does" — same underlying
  cause as the Markdown case (the built-in `dracula` fallback already
  colors these; this hand-built theme didn't), not a build/cwd issue as
  the report first suggested. Scope names taken from the real grammar
  (sublimehq/Packages' `Diff/Diff.sublime-syntax`), not guessed — the
  no-`.diff`-suffix selectors (`markup.inserted`, not
  `markup.inserted.diff`) are deliberate prefix matches, so they also
  cover any other grammar using the same inserted/deleted/changed
  convention, not just this one
- everything else (plain text, punctuation/operators) uses `foreground`
  unmodified — deliberately modest, not an exhaustive TextMate grammar

Built programmatically via `syntect::highlighting::Theme` (accessed as
`edtui::syntect::...` — `edtui` publicly re-exports the whole crate, so
this needed no direct `syntect` dependency of our own), then handed to
`edtui`'s `SyntaxHighlighter::custom_theme()`. `SyntaxHighlighter::new`
is still called first with the named theme regardless — it's also what
resolves the file extension to the actual grammar (`SyntaxReference`)
and bundles the matching `theme_set`/`syntax_set`; only the color
`Theme` inside it gets swapped out.

## Bundled grammars for what `syntect`'s default set is missing

`syntect`'s bundled default syntax set doesn't cover every real-world
extension — confirmed missing: PowerShell (`.ps1`/`.psm1`/`.psd1`), INI
(`.ini`/`.cfg`/`.conf`, and a handful of INI-shaped dotfiles like
`.editorconfig`/`.pylintrc`/`.coveragerc` — see the grammar's own
`hidden_file_extensions` list), TOML (`.toml`, plus `Cargo.lock` and
other lockfiles via *its* `hidden_file_extensions`), Git Ignore
(`.gitignore`), Git Attributes (`.gitattributes`), and Git Config
(`.gitconfig`/`.gitmodules` by name, and plain `.git/config` — no
usable name of its own — by *first line*).
`editor.rs::resolve_syntax_highlighter` is the general-purpose
resolver, tried on every file open:

- `editor.rs::BUNDLED_GRAMMARS` — `.sublime-syntax` (YAML) grammars
  bundled at compile time via `include_str!`, one entry per language:
  - PowerShell: github.com/SublimeText/PowerShell (MIT license — see
    `assets/syntax/PowerShell.LICENSE.txt`).
  - INI: github.com/jwortmann/ini-syntax (Apache-2.0 license — see
    `assets/syntax/INI.LICENSE.txt`). Confirmed missing from
    sublimehq/Packages itself, not just `syntect`'s build of it — INI
    genuinely isn't one of the packages Sublime Text ships by default.
  - TOML, Git Ignore, Git Attributes, Git Config, Git Common: all five
    straight from sublimehq/Packages itself (confirmed present there
    by browsing the repo) — permissively licensed, the exact same
    source `syntect`'s own default bundle is built from, so pulling a
    few more files off it raises no new licensing question
    (`assets/syntax/sublimehq-Packages.LICENSE.txt`). Unlike
    PowerShell/INI, these aren't a *sublimehq/Packages* gap, just
    seemingly missing from `syntect`'s own dump of it for some unknown
    reason.
  - Git Ignore, Git Attributes, and Git Config all `include:` rules
    from the shared `Git Common.sublime-syntax` (`hidden: true` — not
    selectable by extension on its own, only usable as an include
    target). It has to be bundled into the same `SyntaxSet` too, or
    those `include:`s silently resolve to nothing — found by hand, in
    the running app: `.gitignore` opened fine and resolved a
    highlighter, but the file rendered with *zero* color. `syntect`
    doesn't treat an unresolved include as a load error, so nothing
    failed loudly — the bug was only catchable by actually running
    highlighting on real content and checking it colored something,
    not by checking a `SyntaxHighlighter` was merely constructible
    (see `editor::tests::gitignore_comments_are_actually_colored_not_just_resolvable`,
    which fails with `ParsingError(UnresolvedContextReference(..))` if
    `Git Common.sublime-syntax` is ever removed from `BUNDLED_GRAMMARS`
    again).
  - CMake (`CMakeLists.txt` by name, `.cmake` by extension):
    github.com/zyxar/Sublime-CMakeLists (MIT license — see
    `assets/syntax/CMake.LICENSE.txt`). Missing for a different reason
    than PowerShell/INI: Sublime Text has never shipped CMake support
    out of the box at all (it's always been a third-party package), so
    `syntect`'s own bundle — built from what Sublime actually ships —
    doesn't have it either, even though it does exist upstream (unlike
    INI, which genuinely has no source at all). Same
    `include:`-a-hidden-dependency shape as the Git formats above:
    `CMake.sublime-syntax`'s `main` context includes
    `CMakeCommands.sublime-syntax` (scope `commands.builtin.cmake`,
    `hidden: true`) for command-argument highlighting, so both files
    are bundled together — confirmed with the same "actually run
    highlighting and check it colored something" test shape
    (`editor::tests::cmake_commands_include_is_actually_resolved_not_just_present`).
    Reported for `CMakeLists.txt.sdk` template files (this project's
    own build-system naming convention, not a general one) — handled
    by `Editor::view` trying the name/extension again with one trailing
    `.sdk` stripped, rather than teaching the grammar itself about a
    project-specific suffix it has no reason to know about.
  - A custom Rust grammar (github.com/rust-lang/rust-enhanced) was
    tried here too, to get closer to VS Code's own highlighting —
    reverted after two rounds of local patches (widening its type
    coverage, then unifying primitive vs. named types onto one scope)
    still didn't hold up against real-world comparison. `.rs` is back
    to `syntect`'s own bundled grammar, unmodified — see the
    syntax-highlighting architecture note near the top of this file for
    why a static TextMate grammar was never going to fully match VS
    Code's real-analyzer-powered output anyway.
- **Why not github.com/PowerShell/EditorSyntax** for PowerShell (the
  obvious first choice, Microsoft's own repo): it only ships a
  `.tmLanguage` (plist XML) grammar, and `syntect` doesn't load that
  format for syntax definitions at all — `SyntaxDefinition::load_from_str`
  only parses YAML `.sublime-syntax`; `syntect`'s `plist-load` feature
  (on by default) covers `.tmTheme` *color themes*, an entirely
  different thing from `.tmLanguage` *grammars*. Downloaded it,
  discovered this, swapped to the SublimeText/PowerShell source
  instead, which already ships the YAML format — the same
  YAML-not-plist requirement is why the INI source was picked
  carefully too, rather than grabbing the first INI package found.
- `editor.rs::bundled_extra_syntax_set()` loads all of
  `BUNDLED_GRAMMARS` into one shared minimal `SyntaxSet` via
  `SyntaxSetBuilder`, lazily (only once *any* file needing one of them
  is actually opened) — not merged into `edtui`'s own shared default
  `SyntaxSet`, since `syntect::parsing::SyntaxSet` isn't `Clone` and
  there's no cheap way to extend the one `edtui` already loaded without
  reloading the entire default bundle a second time just to add a few
  grammars.
- **`resolve_syntax_highlighter` is a three-tier lookup**: `[file_name,
  extension]` against `syntect`'s own bundled `SYNTAX_SET`, then the
  same two against our `bundled_extra_syntax_set()`, then — only if
  neither matched by name at all — the file's first line
  (`Editor::first_line`, captured once at `open()`) against both sets
  in the same order. Mirrors `syntect`'s own convenience method
  `SyntaxSet::find_syntax_for_file`'s two-tier (name, then first line)
  lookup, just spread across two `SyntaxSet`s instead of one. Landed in
  two passes, from two different reports on the same underlying theme
  ("some file that should highlight doesn't"):
  - **Name-first, not extension-only**: `Path::extension()` returns
    `None` for a dotfile like `.gitignore` (Rust treats a leading dot
    with no further dot as "no extension", not as a hidden file with
    an empty name), so an extension-only lookup silently skipped every
    dotfile regardless of what grammars were bundled — a real bug in
    our own dispatch, not a missing-grammar problem. Explains why some
    grammars list full file names (`Cargo.lock`, `.editorconfig`, ...)
    in their own `hidden_file_extensions`, not just bare extensions.
  - **First-line as a third tier**: added specifically for `.git/config`
    — its `file_name` candidate is just `"config"`, which no grammar
    declares by name, but `GitConfig.sublime-syntax` declares
    `first_line_match: ^\[core\]` for exactly this reason. Requested
    explicitly as a *general* mechanism rather than a one-off special
    case for that one file — this is why it's implemented as a real
    lookup tier using `SyntaxSet::find_syntax_by_first_line`, not a
    hardcoded "if file_name == config" branch: any future grammar
    (bundled here or already in `syntect`'s own set) that identifies
    itself by first line rather than name now just works.
- Any future "extension X has no highlighting" report should check
  `editor::tests::syntect_bundles_rust_but_not_powershell`-style first
  (does `syntect`'s own bundled set actually lack it, like PowerShell/
  INI/TOML/Git formats — or is it present but under-themed, like
  Markdown was — see the `markup.*` scopes above, or unreachable by
  name at all, like `.git/config` above) before assuming a new grammar
  needs bundling at all.

## Explicitly out of scope for now

- No hot-reload of an edited theme file — roadmap stage 5 territory
  (`notify` crate), requires restarting the app to pick up changes.
- Real terminal cursor *color* (as opposed to shape, already themed via
  `crossterm::cursor::SetCursorStyle` in `main.rs`) isn't set from
  `cursorColor` — would need an OSC 12 escape sequence crossterm
  doesn't wrap.
