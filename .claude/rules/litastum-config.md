# litastum: app-wide config, tunable limits, and environment variables

## `theming::config` owns `config.json`, not just themes

The module is named `theming::config` for historical reasons (it
started out holding only `interface_theme`/`editor_theme`), but it's
grown into this app's one shared `config.json` reader/writer —
`active_shell`, `popup_style`, and now `Limits` (below) all live in the
same file, read through the same `read_config`/`persist` machinery.
Not renamed to something more general yet; would touch a lot of
`crate::theming::config::X` call sites for a purely cosmetic win. Revisit
if the "theming" name ever becomes actively misleading rather than just
slightly imprecise.

## `Settings`: the persisted UI choices, one struct

`popup_style`, `editor_keymap_mode` and `compare_line_ending_display`
are one `theming::config::Settings`, kept on `App::settings`:
`load_settings()` reads them in one pass (each missing field falling
back on its own), and a menu that changes one sets the field and calls
`save_settings(&app.settings)`. A new setting of this kind is one field
on `Settings` (and `Config`), not a new load/set pair. Themes and the
shell profile stay separate on purpose: themes are looked up by name
across directories, and the shell is only saved by F9 -> Options ->
Save setup.

## `left_panel_path`/`right_panel_path`: the panels reopen where they were

Requested: litastum starts with each panel in its last directory (one
that no longer exists falls back to the start directory, `main.rs`).
Written (`last_paths.rs`) 30 s after a change -- a crash loses at most
that -- at exit, and when the console is closed: litastum's window or
tab, Windows Terminal's tab, a logoff (`CTRL_CLOSE_EVENT`, handled with
`SetConsoleCtrlHandler`, synchronously -- Windows ends the process when
the handler returns, so `ctrlc`'s handler thread could be too late).
Not on every change (requested). With several tabs, the last write
wins.

## `window_zoom`: litastum's window keeps its zoom here

The window (`gui/src/zoom.rs`) saves each screen's font size (`main`,
`compare`, `conflict` -- litastum reports its screen with the terminal
user variable `litastum_screen`, `event_loop::sync_screen`) under
`window_zoom` in this same `config.json`, editing the file as plain
JSON so every other key stays. The console app doesn't use the field
but declares it on `Config`: every save writes the whole file, and an
unknown key would be dropped (`saving_a_setting_keeps_the_windows_zoom`).

## `Limits` (`theming/config/limits.rs`): tunable caps, one place

Requested directly, after a perf/weak-spot audit pass turned up five
unrelated hardcoded `const`s scattered across the codebase, each only
ever changeable by editing source and rebuilding:

| `Limits` field                | Was                                    | Default     |
|--------------------------------|-----------------------------------------|-------------|
| `max_command_history`          | `command_line/history.rs::MAX_HISTORY`  | 50          |
| `find_file_max_results`        | `find_file/search.rs::MAX_RESULTS`      | 200         |
| `find_file_max_visited`        | `find_file/search.rs::MAX_VISITED`      | 2,000,000   |
| `panel_min_column_width`       | `ui/panel.rs::MIN_COLUMN_WIDTH`         | 24          |
| `markdown_preview_page_size`   | `markdown_preview.rs::PAGE_SIZE`        | 15          |
| `max_log_bytes`                | `logging.rs::MAX_LOG_BYTES`             | 30 MiB      |
| `max_paste_undo_stack`         | (new -- whole-buffer undo entries kept, `editor/editor/undo.rs`) | 20 |

Each is an independent `Option<T>` field on the same `Config` struct
`interface_theme`/`popup_style`/... already use — an unset field keeps
its own hardcoded default (`Limits::default()`), same "missing/
malformed falls back per-field, never blocks the rest" rule every other
setting in this file follows. Not surfaced through any menu yet — hand-
edit `config.json` to override one, the same way `interface_theme`/
`editor_theme` worked before the F9 picker existed for those:

```json
{
  "max_command_history": 200,
  "find_file_max_visited": 500000
}
```

**Centralizing the *values* doesn't change who reads them or how** —
each of the five call sites still reads its own one field straight from
`theming::config::limits()` (a lazily-loaded, process-lifetime-cached
`&'static Limits` — see its own doc comment for why it's cached rather
than re-reading `config.json` on every call: several callers are on
real per-frame paths, `ui::panel::draw_panel` and
`MarkdownPreviewState::page_down`/`page_up` among them). No new
parameter got threaded through any of those five functions' own
signatures; this was a "swap a `const` for a function call" change,
not a "restructure these five modules" one.

**Test isolation**: `limits()` always returns `Limits::default()` in a
test build, unconditionally — same rule `config_dir()` itself already
follows (`config_dir`'s own doc comment), for the same reason: a test's
result must never depend on whatever `config.json` a developer
running the suite actually has. The per-field
overlay logic itself (`resolve_limits`) is still fully unit-tested,
just against a plain in-memory `Config` value, never a real file.

## Environment variables: `LITASTUM_`-prefixed, one place to look

One litastum-specific environment variable exists:
`LITASTUM_HOST_CELL_SIZE` (`image_host.rs`), set by litastum's own
window (`gui/`) for the console app it hosts -- its cell size in pixels,
so F3 images use iTerm2 inline images at that size (ConPTY makes asking
impossible; `docs/history/launching.md`). `LITASTUM_CONFIG_DIR` used to
override the config directory for local development; it went away with
`appdata/` (see "Where app data lives" below), which is already inside
the project.

**Convention for any future one**: prefix it `LITASTUM_` -- avoids colliding with anything else in a user's environment, and
makes it immediately recognizable as this app's own in a `set`/`env`
dump. Declare its literal name as a `pub(crate) const ..._ENV_VAR: &str`
right next to whatever reads it (its doc comment explains *why* the
variable exists, not just what it does), and read it only from
`#[cfg(not(test))]` code, so `cargo test` never depends on whatever a
developer's own shell happens to have set. `RUST_LOG` (read by
`tracing_subscriber::EnvFilter::try_from_default_env()` in
`logging.rs`) is the one exception to the `LITASTUM_` prefix — it's not
litastum-specific at all, it's the standard `tracing` ecosystem
convention every Rust app built on that crate already shares, so
renaming it would just make this app *less* consistent with the
tooling its users likely already know.

## Where app data lives: `appdata/` in the project, for now

There's no installer yet, so the app reads and writes nothing outside
the project. Everything an installed build will keep in
`%APPDATA%\litastum\` lives in `appdata/` at the project root
(`src/app_data.rs::app_data_dir`, anchored with `CARGO_MANIFEST_DIR`),
in the same layout:

```
appdata/
  config.json          settings, themes, shell, Limits overrides
  themes/              user theme files (the repo's themes/ stays the bundled set)
  LitastumMenu.toml    the common F2 menu
  history/             command line, editor search, Find file histories
```

`theming::config::config_dir()` and `app_data::history_dir()` both
derive from it. `appdata/` is git-ignored. `logs/` stays at the project
root for now.

**When the installer lands**: switch `app_data_dir()` to the OS
location (`directories::ProjectDirs`, per [[litastum-stack]]) and move
the directory's contents over as-is. Not before.

