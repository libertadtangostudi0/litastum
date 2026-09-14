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
result must never depend on whatever `config.json`/`LITASTUM_CONFIG_DIR`
a developer running the suite actually has configured. The per-field
overlay logic itself (`resolve_limits`) is still fully unit-tested,
just against a plain in-memory `Config` value, never a real file.

## Environment variables: `LITASTUM_`-prefixed, one place to look

Only one litastum-specific environment variable exists today:
`LITASTUM_CONFIG_DIR` (`theming::config::limits`'s sibling constant
`CONFIG_DIR_ENV_VAR`, in `theming/config/mod.rs`) — overrides the whole
config directory (`config.json`, `themes/`) to any path, added for
local development so testing config-directory-dependent features (the
F2 user menu's common-menu fallback) doesn't mean creating real files
under `%APPDATA%\litastum\` by hand.

**Convention for any future one**: prefix it `LITASTUM_`, same as this
one — avoids colliding with anything else in a user's environment, and
makes it immediately recognizable as this app's own in a `set`/`env`
dump. Declare its literal name as a `pub(crate) const ..._ENV_VAR: &str`
right next to whatever reads it (`CONFIG_DIR_ENV_VAR`'s own doc comment
is the template — explain *why* the variable exists, not just what it
does), and gate it the same way `config_dir()` does: read only from
`#[cfg(not(test))]` code, so `cargo test` never depends on whatever a
developer's own shell happens to have set. `RUST_LOG` (read by
`tracing_subscriber::EnvFilter::try_from_default_env()` in
`logging.rs`) is the one exception to the `LITASTUM_` prefix — it's not
litastum-specific at all, it's the standard `tracing` ecosystem
convention every Rust app built on that crate already shares, so
renaming it would just make this app *less* consistent with the
tooling its users likely already know.
