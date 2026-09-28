# litastum: theming

History of how these decisions were reached (the default-theme switch,
the scope gaps, the grammar dead ends): `docs/history/theming.md`.

## Format: Windows Terminal color scheme JSON, reused as-is

Theme files are exactly Windows Terminal's `colorSchemes` shape --
`name`, `black`..`white` + `bright*` (16 ANSI slots), `background`,
`foreground`, `selectionBackground`, `cursorColor`, all `"#rrggbb"`. A
downloaded file drops straight into `themes/`; parsing is
`scheme.rs::ColorScheme`. Ready-made schemes:
<https://windowsterminalthemes.dev/>.

The out-of-the-box **default is `themes/github-dark-default.json`**,
taken from `@primer/primitives`' own color tokens (what VS Code's
`GitHub Dark Default` is generated from), not a third-party mirror.
`themes/github-dark.json` stays as a separately selectable classic
variant.

## Interface theme and editor theme are independent, like in Far Manager

`config.json` has two keys, `interface_theme` and `editor_theme`, each
naming a theme file on its own -- either can be set without the other,
both can name the same file, or both can be omitted.

## Where files live and how the active one is chosen

- `<OS config dir>/litastum/config.json` (via the `directories` crate:
  `%APPDATA%\litastum\`, `~/.config/litastum/`, `~/Library/Application
  Support/litastum/`), each key naming a file stem in `themes/`.
- Theme files are searched in the config dir's `themes/`, then in
  `./themes/` relative to the cwd (`config.rs::theme_search_dirs`) --
  `config.json` itself is not searched that way.
- Anything missing or broken falls back **per half** to
  `config.rs::default_theme` (the bundled default above), and one level
  further to the hardcoded `Theme::dark()`/`SYNTAX_THEME` ("dracula") if
  even that fails. Never blocks startup or panics;
  `config.rs::load_active_theme` is the one entry point, called once in
  `main()`. `warn!` for "something there but broken", `debug!` for
  "nothing configured".
- `themes/apple-system-colors.json` is a known-working example and the
  fixture `scheme.rs`'s tests parse.
- **F9 → Options → Color schemes** (`theme_menu.rs`): `Enter` applies
  the highlighted theme to both halves, `I`/`E` to just one. Applies
  live and persists to `config.json`; a failed write is logged and
  doesn't block the live preview.

## Mapping conventions (decided unilaterally, easy to revisit)

Windows Terminal schemes have no "accent"/"border"/"danger" concept, so
`scheme.rs::ColorScheme::to_theme` maps:

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
| `command_line_prefix` | `commandLinePrefix` (litastum-specific, below), falls back to `accent` |
| `selection_text`   | `selectionForeground` (litastum-specific, below), `None` = leave text color alone |

### Two litastum-specific, optional keys

Neither exists in a plain Windows Terminal scheme, and neither is
needed -- both are `#[serde(default)]`:

- **`commandLinePrefix`** -- the command line's `"{cwd}> "` prefix
  color, for a color no ANSI slot holds (Far's own terracotta
  `#d7875f` in `far-lts-alien.json`). Absent means `accent`. Rendered
  bold, like Far's own flag on that color group.
- **`selectionForeground`** -- text color over a selection/selected
  row, for a scheme whose `selectionBackground` is bright enough to
  need it (`molocai`, `ubuntu`, `alien-blood`, `dracula` use it).
  Absent means *leave the text color alone* -- forcing one color would
  undo file-type coloring for every other scheme.

Every selected row / in-place text selection goes through
`ui/popup.rs::{selected_row_style, selected_text_style}` so this
reaches every popup at once; the one exception is the Markdown
preview's highlighted line (`ui/markdown_preview.rs::render_line`),
which layers the background over per-span colors and applies the same
override in place.

## File-type coloring (`panel.rs::HighlightRole`)

Far Manager-style, deliberately small -- a handful of extensions, not
Far's regex-based `highlighting.hgh`:

| `HighlightRole`   | Matches                                              | Color             |
|--------------------|-------------------------------------------------------|--------------------|
| `Parent`           | `..`                                                   | `text_dim`         |
| `Directory`        | an ordinary directory                                  | `text` (no distinction) |
| `VcsDirectory`     | `.git .svn .hg .bzr`                                   | `accent`           |
| `Archive`          | `.zip .7z .rar .tar .gz .bz2 .xz`                      | `warning`          |
| `Executable`       | `.exe .bat .cmd .sh .ps1 .py .js .ts .rb .pl`          | `success`          |
| `Other`            | everything else                                        | `text`             |

Ordinary directories are *not* colored (matches a real Far
screenshot). The colors are a first pass -- easy to retune in
`panel.rs::Entry::highlight_role` and `ui::build_list_item`.

## Syntax highlighting: base16-style derivation

With `editor_theme` set, the `syntect` theme is derived from the
scheme's 16 colors, base16-style (`scheme.rs::ColorScheme::to_syntax_theme`);
without it, the named `SYNTAX_THEME` ("dracula") is used, independent
of `interface_theme`. Built as a `syntect::highlighting::Theme` (via
`edtui::syntect`, re-exported -- no direct `syntect` dependency) and
handed to `SyntaxHighlighter::custom_theme()`; `SyntaxHighlighter::new`
still resolves the grammar.

- `keyword`, `storage` → `purple`; `string` → `green`; `comment` →
  `brightBlack`; `constant.numeric`/`constant.language` → `yellow`
- `entity.name.function`, `support.function` → `blue`;
  `entity.name.type`/`class`, `support.type` → `cyan`;
  `variable.parameter`, `entity.name.tag` → `red`
- Markdown: `markup.heading` (bold) → `cyan`, `markup.bold` (bold) →
  `yellow`, `markup.italic`/`markup.quote` (italic) → `purple`/
  `brightBlack`, lists → `red`, links → `blue`, code → `green`, their
  punctuation markers → `brightBlack`
- Diff: `markup.inserted` → `green`, `markup.deleted` → `red`,
  `markup.changed` → `yellow`, `meta.diff.header`/`meta.header` (bold)
  → `cyan`, `meta.diff.range`/`punctuation.definition.range` → `blue`,
  `meta.separator.diff` → `brightBlack` (un-suffixed selectors on
  purpose -- prefix matches cover other grammars too)
- everything else uses `foreground` unmodified

**Any scope family a grammar emits needs a mapping here**, or it
renders plain under a custom scheme while looking fine under the
`dracula` fallback -- that's exactly how both the Markdown and the Diff
gaps showed up.

### Bundled grammars for what `syntect`'s default set is missing

`editor.rs::BUNDLED_GRAMMARS` -- YAML `.sublime-syntax` files compiled
in with `include_str!`, loaded lazily into one extra `SyntaxSet`
(`bundled_extra_syntax_set()`; `edtui`'s own set can't be extended,
`SyntaxSet` isn't `Clone`):

| Grammar | Source (license in `assets/syntax/`) |
|---|---|
| PowerShell | github.com/SublimeText/PowerShell (MIT) |
| INI (+ `.editorconfig`, `.pylintrc`, ...) | github.com/jwortmann/ini-syntax (Apache-2.0) |
| TOML (+ `Cargo.lock`), Git Ignore, Git Attributes, Git Config, Git Common | sublimehq/Packages |
| Groovy (`.groovy`, `.gvy`, `.gradle`, `Jenkinsfile`) | sublimehq/Packages |
| CMake (`CMakeLists.txt`, `.cmake`) + CMakeCommands | github.com/zyxar/Sublime-CMakeLists (MIT) |
| DCL (AutoCAD Dialog Control Language, `.dcl`) | self-authored, deliberately modest -- the only public `.dcl` grammar is OpenVMS's unrelated language |

`editor/syntax/grammars.rs::EXTENSION_ALIASES` maps an extension onto
an existing grammar instead of bundling one (`.rc`/`.rc2` -> C++,
`.clang-format`/`.clang-tidy` -> YAML) -- also the way to avoid an
unrelated grammar that merely shares an extension (Android's
`init.rc`).

- Only YAML `.sublime-syntax` works -- `syntect` doesn't load
  `.tmLanguage` grammars.
- A grammar that `include:`s a hidden one needs that one bundled too
  (Git Common, CMakeCommands), or it silently colors nothing -- test
  by running real highlighting, not by constructing a highlighter.
- `.rs` uses `syntect`'s own grammar, unmodified.

### How a file's grammar is found

`resolve_syntax_highlighter` tries, in order: `[file_name, extension]`
in `syntect`'s own set, the same in the bundled set, then the file's
first line (`Editor::first_line`) in both -- a general mechanism, which
is how `.git/config` (`first_line_match: ^\[core\]`) gets highlighted.
A trailing `.sdk` is also retried stripped (`CMakeLists.txt.sdk`
templates).

A future "extension X has no highlighting" report: first check whether
`syntect` lacks the grammar, has it but the scopes aren't mapped above,
or can't reach it by name -- before bundling anything.

## Explicitly out of scope for now

- No hot-reload of an edited theme file — roadmap stage 5 (`notify`).
- Real terminal cursor *color* isn't set from `cursorColor` -- would
  need an OSC 12 escape sequence crossterm doesn't wrap (shape is
  themed via `SetCursorStyle` in `terminal_setup.rs`).
