# Theming and syntax highlighting -- history

Current decisions: `.claude/rules/litastum-theming.md`. This is the
record of how they were reached -- mostly real reports, several of
them a fix that turned out to have a second, hidden gap behind it.

## The default theme: `github-dark` -> `github-dark-default`

`themes/github-dark.json` came from `mbadolato/iTerm2-Color-Schemes`'
Windows Terminal mirror, with `background`/`foreground`/
`selectionBackground`/`cursorColor` hand-adjusted toward `#0d1117`/
`#e6edf3` (the "color 2" values `Theme::dark()` already used). Compared
side by side, it didn't match the user's real VS Code theme: VS Code's
GitHub extension ships several different dark palettes, and the one
most people actually have (the extension's own default) is `GitHub
Dark Default`, not the older `GitHub Dark`.

`themes/github-dark-default.json` went straight to the source instead:
every value from `@primer/primitives`' published color tokens
(`dist/docs/functional/themes/dark.json`, which the VS Code theme is
generated from) -- genuinely different values (`foreground` `#c9d1d9`,
not `#e6edf3`; every ANSI color shifted). `selectionBackground` is
computed the way VS Code's own theme source does it: `fgColor-accent`
(`#58a6ff`) at 20% alpha over `background`. Made the default on request
("хочу добавить и сделать её дефолтной"); `github-dark.json` stays as a
separately selectable classic variant.

## One shared `theme` key -> independent `interface_theme`/`editor_theme`

The first version had one `theme` key driving both the interface and
the editor's syntax colors. Using it showed that's not what "customize
the interface" means once the editor can be customized separately --
forcing them to share a file was the bug. Split into two keys, the way
Far Manager keeps its interface scheme and editor highlighting
independent.

## `./themes/` as a search fallback

Theme files were first looked up only in the config directory. A theme
sitting right in the file panel (the repo's own
`themes/apple-system-colors.json`, cwd = repo root under `cargo run`)
didn't show up in the F9 picker -- copying a file before you can even
preview it is backwards. `./themes/` became a fallback after the config
dir. `cargo test`'s cwd is also the package root, so this path has real
test coverage (`config.rs`'s `*_via_cwd_fallback` tests).

## `config_dir()` is `None` in tests

`LITASTUM_CONFIG_DIR` was added for local development (testing the F2
menu's common-directory fallback without creating files under
`%APPDATA%`). The first time it was actually set to a directory with a
real menu, an integration test
(`open_user_menu_tests::creates_and_opens_a_new_menu_file_when_neither_exists`)
started failing: the new fallback routed it through the real config
directory. Every other config-touching function had always been kept
out of tests for exactly this reason, so the gate went into
`config_dir()` itself -- the one choke point -- rather than into each
affected test. `limits()` follows the same rule.

## `success`/`warning` in `Theme`

Planned in the original "color 2" design (`litastum-ui-theme.md`) but
only `danger` landed at first; added once file-type coloring needed
them.

## `commandLinePrefix`

Reproducing real Far Manager's own scheme (`far-lts-alien.json`,
extracted from a real Far install's `colors.db` + console palette, its
`CommandLine.Prefix` color group) needed a bold terracotta `#d7875f`
that isn't any of the 16 ANSI colors -- no principled way to derive it
like `accent`/`danger`. Hence one optional, non-standard key; it falls
back to `accent`, which is what every scheme had before the field
existed. Rendered bold, matching Far's own `[x] Bold` flag on that group.

## `selectionForeground`, and the two gaps behind it

Added for `themes/molocai.json`: its `selectionBackground` was changed
from Molokai's own pale blue `#b5d5ff` to its bright ANSI `green`
`#98e123` (liked from the theme's demo on windowsterminalthemes.dev --
actually the swatch used for a Jest diff highlight there), with black
text to keep it readable ("внутри чёрный текст"). `ubuntu.json`,
`alien-blood.json` and `dracula.json` got the same treatment, each
picking a vivid color from its own palette (`brightPurple`,
`brightGreen`, Dracula's iconic `green`) instead of the generic
`#b5d5ff` many community schemes never bothered to customize.

1. **Every other popup ignored it.** The panel's selected row showed
   black-on-green correctly, but `ui/menu.rs`, the theme pickers,
   `find_file.rs`, the history popups, the Copy/Move field, ... each had
   its own `Style::default().fg(theme.text).bg(theme.current_row_bg)`
   literal -- reported from a screenshot of the F9 menu. Centralized
   into `ui/popup.rs::{selected_row_style, selected_text_style}`.
2. **The Markdown preview's own highlighted line** still showed light
   text on green next to the editor showing black on the same line. It
   layers `current_row_bg` over each span's own kind-based color, which
   neither helper's fixed starting color fits -- fixed in place in
   `ui/markdown_preview.rs::render_line`.

## File-type coloring

Reverses an earlier "no file-type color dots, deliberate minimalism"
decision, on request. The first version colored every directory; a
real Far Manager screenshot showed ordinary directories in plain text
and only `.git` colored distinctly -- generalized to `VcsDirectory`
(common VCS metadata dirs). Only the *rule* is borrowed from Far; the
color comes from the active scheme. The category -> color choices are a
first pass, not researched against Far's `highlighting.hgh`.

## Syntax scopes that fell through to plain text

The derived syntax theme originally named code scopes only.

- **Markdown "has no highlighting" under a custom `editor_theme`.** The
  highlighter was running (`syntect` does bundle Markdown), but every
  `markup.*` scope fell through to plain `foreground`. The built-in
  `dracula` fallback has its own `markup.*` rules, which is why this
  only showed with a custom scheme. Added the `markup.*` mappings.
- **`.diff`/`.patch`, the same gap again**, reported as "works with an
  older build without `editor_theme`, not with one" -- first suspected
  as a build/cwd issue, but the cause was identical. `syntect` already
  resolves a real Diff grammar; added `markup.inserted/deleted/changed`
  and `meta.diff.*`. Scope names taken from sublimehq/Packages'
  `Diff/Diff.sublime-syntax`, not guessed; the un-suffixed selectors
  are deliberate prefix matches so other grammars using the same
  convention are covered too.

## Bundled grammars

- **PowerShell: why not github.com/PowerShell/EditorSyntax** (the
  obvious choice, Microsoft's own): it only ships a `.tmLanguage`
  (plist XML) grammar, and `syntect` only loads YAML `.sublime-syntax`
  for grammars -- its `plist-load` feature covers `.tmTheme` *color
  themes*, not `.tmLanguage` *grammars*. Downloaded it, found this,
  switched to SublimeText/PowerShell. The same YAML-only requirement is
  why the INI source was picked carefully.
- **INI** genuinely isn't in sublimehq/Packages at all (not just
  missing from `syntect`'s build of it). TOML and the Git formats *are*
  in sublimehq/Packages, just missing from `syntect`'s dump for an
  unknown reason.
- **Git Common, the silent `include:`.** `.gitignore` opened fine and
  resolved a highlighter, but rendered with zero color: Git Ignore/
  Attributes/Config all `include:` the hidden `Git Common.sublime-syntax`,
  and `syntect` doesn't treat an unresolved include as a load error.
  Found by hand in the running app; only catchable by running real
  highlighting and checking something got colored, not by checking a
  highlighter was constructible
  (`editor::tests::gitignore_comments_are_actually_colored_not_just_resolvable`).
- **CMake** has never been shipped with Sublime (always a third-party
  package), so `syntect` lacks it even though a source exists. Same
  hidden-include shape as Git Common (`CMakeCommands.sublime-syntax`),
  same kind of test. Reported for `CMakeLists.txt.sdk` templates (this
  project's own build naming) -- handled by retrying the name with one
  `.sdk` stripped, rather than teaching the grammar about it.
- **A custom Rust grammar** (github.com/rust-lang/rust-enhanced), tried
  to get closer to VS Code, was reverted after two rounds of local
  patches (widening type coverage, then unifying primitive vs. named
  types onto one scope) still didn't hold up against real comparison.
  A static TextMate grammar can't match VS Code's analyzer-driven
  highlighting anyway.
- **DCL** (AutoCAD Dialog Control Language), reported for a real
  `base.dcl`: the only public `.dcl` grammar is OpenVMS's unrelated
  DIGITAL Command Language, which would mis-highlight, so it's
  self-authored and deliberately modest (comments, strings, numbers,
  punctuation, tile types, attribute names).
- **Groovy** (`Jenkinsfile` via `hidden_file_extensions`) is in
  sublimehq/Packages but not in `syntect`'s dump, like TOML. One local
  patch: the upstream `comments` context tried `include:
  scope:text.html.javadoc` first -- a cross-grammar reference to
  Javadoc, which isn't bundled. Reported on a real `/** ... */` block:
  the first and last lines colored, every line between rendered as code.
  The line was removed; the plain comment-block fallback is sufficient
  (`groovy_multiline_doc_comment_colors_every_line_as_comment`).
- **Aliases instead of grammars** (`EXTENSION_ALIASES`). `.rc`/`.rc2`
  (`TBVersionInfo.rc2`, reported unhighlighted) are mostly C
  preprocessor directives plus a few RC keywords, so C++ gets most of
  it; the only public `.rc` grammar is Android's unrelated `init.rc`.
  `.clang-format`/`.clang-tidy` (reported as a wall of plain text) have
  no extension by `Path::extension()`'s rules, but both are real YAML,
  so the alias matches the full file name.

## Resolver tiers

`resolve_syntax_highlighter` landed in two passes, from two reports of
"a file that should highlight doesn't":

- **Name first, not extension only.** `Path::extension()` is `None` for
  a dotfile like `.gitignore`, so an extension-only lookup skipped every
  dotfile regardless of bundled grammars -- a bug in our own dispatch,
  not a missing grammar.
- **First line as a third tier**, for `.git/config` (its name is just
  `config`, but `GitConfig.sublime-syntax` declares
  `first_line_match: ^\[core\]`). Requested as a general mechanism, not
  an `if file_name == "config"` special case.
