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
  step falls back to that *half's* built-in default only — a broken
  `editor_theme` doesn't take the interface theme down with it, and
  vice versa. Logged (`warn!` for "something was there but broken",
  `debug!` for "nothing configured" — see `.claude/rules/logging.md`'s
  level semantics). Never blocks startup or panics —
  `config.rs::load_active_theme` is the one entry point, called once in
  `main()`.
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
- **F9 → Settings → Color schemes** opens `theme_menu.rs` (`menu.rs`
  is the F9 top menu itself — not Far Manager's real top-menu bar,
  just enough structure to reach this): lists whatever
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

`success`/`warning` were in the original "color 2" plan
(`.claude/rules/litastum-ui-theme.md`) but only `danger` actually
landed in `Theme` initially — added properly once file-type coloring
below needed them.

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

## Explicitly out of scope for now

- No hot-reload of an edited theme file — roadmap stage 5 territory
  (`notify` crate), requires restarting the app to pick up changes.
- Real terminal cursor *color* (as opposed to shape, already themed via
  `crossterm::cursor::SetCursorStyle` in `main.rs`) isn't set from
  `cursorColor` — would need an OSC 12 escape sequence crossterm
  doesn't wrap.
