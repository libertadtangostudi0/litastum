use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(not(test))]
use directories::ProjectDirs;
use edtui::syntect::highlighting::Theme as SynTheme;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use super::popup_style::PopupStyle;
use super::scheme::ColorScheme;
use super::theme::Theme;
use crate::editor::EditorKeymapMode;

mod limits;
pub use limits::limits;


/// Overrides `config_dir()`'s own OS-default location entirely when
/// set, to any directory (not necessarily one named `litastum` at all).
/// Added directly for local development: testing the F2 user menu's
/// common-directory fallback (`explorer::user_menu::state::resolve_menu`)
/// against the real `%APPDATA%\litastum\` meant creating a config
/// there by hand every time just to exercise the feature, when the
/// project's own checkout (`W:\rust\litastum`) was right there and
/// easier to inspect/clean up -- `pub(crate)` (not `pub`) since only
/// this crate's own code ever needs to resolve the config directory,
/// never something outside it.
// Only read from `#[cfg(not(test))]` code below (`config_dir` is always
// `None` in a test build, so it never even looks at this) -- the lint
// is right that a test build genuinely never uses it, not a sign of
// dead code in the real, non-test build.
#[cfg_attr(test, allow(dead_code))]
pub(crate) const CONFIG_DIR_ENV_VAR: &str = "LITASTUM_CONFIG_DIR";

/// This app's config directory. Normally `<OS config dir>/litastum/`,
/// if the platform gives us one at all (some CI/headless environments
/// don't report a home directory — that's not fatal, it just means no
/// custom theme/menu is possible, same as if the directory were simply
/// empty) — overridden wholesale by `LITASTUM_CONFIG_DIR` when that's
/// set, see its own doc comment above for why.
///
/// **Always `None` in a test build** (`cfg(test)`), unconditionally --
/// neither the real OS config directory nor `LITASTUM_CONFIG_DIR` is
/// consulted at all. Every other function in this app that touches the
/// real config directory has deliberately never been called from the
/// test suite for exactly this reason (`theming::config::tests`' own
/// comment: exercising the real path would make a test's result depend
/// on whatever a developer running the suite actually has sitting
/// there, config.json/LitastumMenu.toml included, rather than the code
/// under test). `resolve_menu`'s new common-menu fallback broke that
/// invariant by routing a real integration test
/// (`explorer::command::tests::open_user_menu_tests::
/// creates_and_opens_a_new_menu_file_when_neither_exists`) through this
/// function for the first time -- it started failing the moment
/// `LITASTUM_CONFIG_DIR` was actually set to a directory with a real
/// menu in it (i.e. the moment the feature this env var exists for was
/// being tested by hand). Gating here, at the one shared choke point,
/// restores that invariant for every current and future caller rather
/// than patching each affected test individually.
pub(crate) fn config_dir() -> Option<PathBuf> {
    #[cfg(test)]
    {
        None
    }
    #[cfg(not(test))]
    {
        if let Ok(dir) = std::env::var(CONFIG_DIR_ENV_VAR) {
            return Some(PathBuf::from(dir));
        }
        ProjectDirs::from("", "", "litastum").map(|dirs| dirs.config_dir().to_path_buf())
    }
}


/// Directories searched for theme *files*, in priority order: the
/// user's own config dir first, then `themes/` relative to the current
/// working directory as a fallback. The fallback exists so the
/// repo-bundled `themes/apple-system-colors.json` example (and
/// anything else dropped next to wherever the app is actually run
/// from — `cargo run` from the repo root, a portable install, ...) is
/// found without first having to copy it into the config dir; found
/// the hard way when a theme sitting right there in the file panel
/// didn't show up in the F9 picker.
///
/// `config.json` itself is *not* searched this way — only theme files.
/// Where the active choice is recorded stays exactly the OS config dir,
/// unambiguously.
fn theme_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = config_dir() {
        dirs.push(dir.join("themes"));
    }
    dirs.push(PathBuf::from("themes"));
    dirs
}


/// Finds and parses `name` across `theme_search_dirs()`, in order — the
/// first directory with a matching, parseable file wins. Logs a single
/// `warn!` only if no directory had it at all (each individual miss
/// while searching is expected, not worth its own warning).
pub fn find_scheme(name: &str) -> Option<ColorScheme> {
    for dir in theme_search_dirs() {
        if let Some(scheme) = load_scheme(&dir, name) {
            return Some(scheme);
        }
    }
    warn!(name, "theme not found in any of the searched directories");
    None
}


/// litastum's own settings file (`config.json` in the config dir).
/// JSON, not TOML — this project already needs `serde_json` for the
/// Windows Terminal-format theme files, so using it here too avoids a
/// second parsing library for one small file.
///
/// `interface_theme` and `editor_theme` are deliberately two separate
/// keys, not one shared `theme` — Far Manager (this project's
/// namesake) keeps its interface color scheme and its editor syntax
/// highlighting as two entirely independent, independently-selected
/// systems, never coupled to each other. Each key names a file in the
/// `themes/` subdirectory (filename stem, no `.json`) and either can be
/// set without the other, or both can point at the same file if a user
/// *does* want one scheme driving everything.
#[derive(Debug, Deserialize, Serialize, Default)]
struct Config {
    interface_theme: Option<String>,
    editor_theme: Option<String>,
    /// The `shell.rs::ShellProfile::name` last chosen via F9 → Commands
    /// → ... wait, → Options → Save setup (Far Manager's own Shift+F9
    /// "save setup" — persisting the current session's choices on
    /// demand, rather than every choice auto-persisting the moment it's
    /// made, the way the theme picker's own choices do). Applied back
    /// at startup in `main.rs` if it names a profile that still exists.
    active_shell: Option<String>,
    /// F9 -> Options -> UI's own choice (`PopupStyle`) -- unlike
    /// `interface_theme`/`editor_theme`, this doesn't name an external
    /// file to search for, so it's stored (and read back) directly as
    /// the enum's own serde representation rather than through
    /// `find_scheme`'s lookup machinery.
    popup_style: Option<PopupStyle>,
    /// The built-in editor's own F9 -> Keybindings choice
    /// (`editor::EditorKeymapMode`) -- same "store the enum's own serde
    /// representation directly, no external file lookup needed" shape
    /// as `popup_style` right above.
    editor_keymap_mode: Option<EditorKeymapMode>,
    /// The six fields below back `Limits` (`limits.rs`) -- optional
    /// overrides for app-wide tunable caps, each independent of the
    /// others (an unset field keeps `Limits::default()`'s own value for
    /// just that one field, same "missing/malformed falls back
    /// per-half, never blocks the rest" rule every other setting in
    /// this file follows). Not currently surfaced through any menu --
    /// hand-edit `config.json` to set one, same as this project's very
    /// first config keys (`interface_theme`/`editor_theme`) worked
    /// before the F9 picker existed for those.
    max_command_history: Option<usize>,
    find_file_max_results: Option<usize>,
    find_file_max_visited: Option<usize>,
    panel_min_column_width: Option<u16>,
    markdown_preview_page_size: Option<usize>,
    max_log_bytes: Option<u64>,
}


/// Loads the active themes: our own UI palette (panels, borders, F-key
/// bar, ...) and, independently, the editor's syntax-highlighting
/// theme — see `.claude/rules/litastum-theming.md`.
///
/// Never fails and never blocks startup: a missing config directory, a
/// missing or malformed `config.json`, a missing or malformed theme
/// file — all of these fall back to the built-in default for that half
/// only (the other half keeps working normally), logged for anyone who
/// wants to know why their theme didn't apply.
pub fn load_active_theme() -> (Theme, Option<SynTheme>) {
    let Some(config_dir) = config_dir() else {
        debug!("no config directory available on this platform; using built-in theme");
        return default_theme();
    };

    let config = read_config(&config_dir);

    let theme = config
        .interface_theme
        .as_deref()
        .and_then(find_scheme)
        .map(|scheme| scheme.to_theme())
        .unwrap_or_else(|| default_theme().0);

    let syntax_theme = config
        .editor_theme
        .as_deref()
        .and_then(find_scheme)
        .map(|scheme| scheme.to_syntax_theme())
        .or_else(|| default_theme().1);

    (theme, syntax_theme)
}


/// The out-of-the-box default, when nothing is configured (or whatever
/// *is* configured fails to load) — the bundled
/// `themes/github-dark-default.json` scheme, for both halves. Falls
/// back further to the hardcoded `Theme::dark()`/`syntax::SYNTAX_THEME`
/// ("dracula") only if that file is itself somehow missing or fails to
/// parse — same "never blocks startup, always degrade gracefully" rule
/// as the rest of this module, just one level deeper than before (the
/// bundled default theme used to just *be* `Theme::dark()`/`None`; now
/// it's a real theme file, so loading it can fail the same way a
/// user-configured one can).
///
/// **"github-dark-default", not "github-dark"**: VS Code's own GitHub
/// theme extension ships several dark variants (`GitHub Dark`, `GitHub
/// Dark Default`, `GitHub Dark Dimmed`, `GitHub Dark High Contrast`,
/// ...) that are genuinely different palettes, not just naming — a
/// first pass at this bundled `github-dark.json` from a third-party
/// terminal-scheme mirror (`mbadolato/iTerm2-Color-Schemes`) turned out
/// to notably mismatch the user's own real VS Code theme, "GitHub Dark
/// Default" specifically (confirmed directly against `@primer/primitives`'
/// own published color tokens, the actual source of truth VS Code's
/// theme is generated from — e.g. `foreground` is `#c9d1d9` there, not
/// `#e6edf3`, and every ANSI color is shifted). `github-dark.json`
/// itself is kept around, unchanged, as a separate, still-selectable
/// theme (F9 → Options → Color schemes) for the classic variant — this
/// function just no longer treats it as *the* default.
fn default_theme() -> (Theme, Option<SynTheme>) {
    match find_scheme("github-dark-default") {
        Some(scheme) => (scheme.to_theme(), Some(scheme.to_syntax_theme())),
        None => {
            warn!("bundled default theme \"github-dark-default\" not found; falling back to the hardcoded built-in theme");
            (Theme::dark(), None)
        }
    }
}


/// The raw `interface_theme`/`editor_theme` names currently configured
/// (if any) -- unlike `load_active_theme`, which resolves them into an
/// actual `Theme`/`SynTheme`, this is just the names, for the F9
/// color-scheme picker to mark whichever entry matches as "current"
/// (`ui/theme_menu.rs`). Same snapshot-at-open, not live, caveat as
/// `ThemeMenu::open`'s own doc comment for `themes`.
pub fn active_theme_names() -> (Option<String>, Option<String>) {
    let Some(config_dir) = config_dir() else {
        return (None, None);
    };
    let config = read_config(&config_dir);
    (config.interface_theme, config.editor_theme)
}


/// Reads and parses `config.json`, falling back to an all-`None`
/// `Config` (i.e. built-in defaults for both halves) on any failure.
fn read_config(config_dir: &Path) -> Config {
    let config_path = config_dir.join("config.json");
    let json = match fs::read_to_string(&config_path) {
        Ok(json) => json,
        Err(_) => {
            debug!(path = %config_path.display(), "no config.json; using built-in defaults");
            return Config::default();
        }
    };

    match serde_json::from_str(&json) {
        Ok(config) => config,
        Err(err) => {
            warn!(path = %config_path.display(), %err, "config.json failed to parse; using built-in defaults");
            Config::default()
        }
    }
}


/// Reads and parses `<dir>/themes/<name>.json`. `None` on any failure
/// (missing file, malformed JSON) — a missing file only logs at
/// `debug!` (callers routinely check several directories, so one not
/// having it is normal, not a problem to flag); a file that exists but
/// fails to parse logs at `warn!` regardless of which directory it's
/// in, since that's an actual broken file.
fn load_scheme(dir: &Path, name: &str) -> Option<ColorScheme> {
    let theme_path = dir.join(format!("{name}.json"));
    let json = match fs::read_to_string(&theme_path) {
        Ok(json) => json,
        Err(_) => {
            debug!(path = %theme_path.display(), "no theme file here");
            return None;
        }
    };

    match ColorScheme::from_json_str(&json) {
        Ok(scheme) => {
            debug!(path = %theme_path.display(), "loaded theme file");
            Some(scheme)
        }
        Err(err) => {
            warn!(path = %theme_path.display(), %err, "configured theme file failed to parse");
            None
        }
    }
}


/// Filename stems of every `*.json` theme file found across
/// `theme_search_dirs()`, deduplicated and sorted — what the F9
/// theme-picker menu lists. Empty (not an error) if nothing is found
/// anywhere; same "nothing configured yet" case as everything else in
/// this module.
pub fn list_theme_names() -> Vec<String> {
    let mut names = std::collections::BTreeSet::new();

    for dir in theme_search_dirs() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(|entry| entry.ok()) {
            if entry.path().extension().is_some_and(|ext| ext == "json") {
                if let Some(stem) = entry.path().file_stem() {
                    names.insert(stem.to_string_lossy().into_owned());
                }
            }
        }
    }

    names.into_iter().collect()
}


/// Applies `name` as the interface theme: persists the choice to
/// `config.json` (best-effort — a write failure is logged but doesn't
/// stop the live theme from applying, since previewing a theme is
/// still useful even if it can't be saved) and returns the resulting
/// `Theme` for the caller to apply immediately. `None` if the named
/// theme file itself can't be found/parsed anywhere in
/// `theme_search_dirs()` (it was in `list_theme_names`'s snapshot but
/// got removed/broken since).
pub fn set_interface_theme(name: &str) -> Option<Theme> {
    let scheme = find_scheme(name)?;
    if let Some(config_dir) = config_dir() {
        persist(&config_dir, |config| config.interface_theme = Some(name.to_string()));
    }
    Some(scheme.to_theme())
}


/// The editor-theme counterpart to `set_interface_theme` — same
/// persistence and fallback behavior.
pub fn set_editor_theme(name: &str) -> Option<SynTheme> {
    let scheme = find_scheme(name)?;
    if let Some(config_dir) = config_dir() {
        persist(&config_dir, |config| config.editor_theme = Some(name.to_string()));
    }
    Some(scheme.to_syntax_theme())
}


/// Far Manager's own "Save setup" (Shift+F9): persists `shell_profile_name`
/// (`app.shell_profiles[app.active_shell].name`) as the shell to start
/// up with next time — best-effort, same as the theme picker's own
/// persistence (a write failure is logged but doesn't block anything,
/// there's no "live preview" to protect here since the choice already
/// took effect this session). See `load_active_shell` for the other
/// half.
pub fn save_setup(shell_profile_name: &str) {
    if let Some(config_dir) = config_dir() {
        persist(&config_dir, |config| config.active_shell = Some(shell_profile_name.to_string()));
    }
}


/// The shell profile name saved by `save_setup`, if any — `main.rs`
/// looks this up once at startup and applies it if a profile by that
/// name still exists (`shell.rs::builtin_profiles()` could have changed
/// between runs, e.g. after an OS upgrade removes `powershell` in favor
/// of `pwsh`; a stale name just falls back to the default, index 0).
pub fn load_active_shell() -> Option<String> {
    let config_dir = config_dir()?;
    read_config(&config_dir).active_shell
}


/// The popup chrome style configured via F9 -> Options -> UI, or
/// `PopupStyle::default()` (`Rounded`) if nothing's configured yet or
/// the config dir/file itself is unavailable -- same "never blocks
/// startup, degrade to a sensible default" rule as `load_active_theme`.
pub fn load_active_popup_style() -> PopupStyle {
    let Some(config_dir) = config_dir() else {
        debug!("no config directory available on this platform; using default popup style");
        return PopupStyle::default();
    };
    read_config(&config_dir).popup_style.unwrap_or_default()
}


/// Persists `style` as the active popup chrome (best-effort, same as
/// `set_interface_theme`/`save_setup` -- a write failure is logged but
/// doesn't stop the live preview from applying, since `app.popup_style`
/// is already set by the caller regardless of whether this succeeds).
pub fn set_popup_style(style: PopupStyle) {
    if let Some(config_dir) = config_dir() {
        persist(&config_dir, |config| config.popup_style = Some(style));
    }
}


/// The built-in editor's own key-binding scheme, configured via its F9
/// menu, or `EditorKeymapMode::default()` (`Standard`) if nothing's
/// configured yet or the config dir/file itself is unavailable -- same
/// "never blocks startup, degrade to a sensible default" rule as
/// `load_active_popup_style`.
pub fn load_active_editor_keymap_mode() -> EditorKeymapMode {
    let Some(config_dir) = config_dir() else {
        debug!("no config directory available on this platform; using default editor keymap mode");
        return EditorKeymapMode::default();
    };
    read_config(&config_dir).editor_keymap_mode.unwrap_or_default()
}


/// Persists `mode` as the default key-binding scheme new editor sessions
/// open with (best-effort, same as `set_popup_style` right above -- a
/// write failure is logged but doesn't stop the live switch from
/// applying, since the caller has already updated both `app.editor_keymap_mode`
/// and the currently-open `Editor` regardless of whether this succeeds).
pub fn set_editor_keymap_mode(mode: EditorKeymapMode) {
    if let Some(config_dir) = config_dir() {
        persist(&config_dir, |config| config.editor_keymap_mode = Some(mode));
    }
}


/// Reads the current `config.json` (or defaults, if there isn't one
/// yet), applies `mutate`, and writes it back. Logs and gives up
/// quietly on any I/O/serialization failure — never panics, and the
/// caller applies the theme live regardless of whether this succeeded.
fn persist(config_dir: &Path, mutate: impl FnOnce(&mut Config)) {
    if let Err(err) = try_persist(config_dir, mutate) {
        warn!(%err, "failed to save theme choice to config.json (theme is still applied for this session)");
    }
}

fn try_persist(config_dir: &Path, mutate: impl FnOnce(&mut Config)) -> io::Result<()> {
    fs::create_dir_all(config_dir)?;
    let mut config = read_config(config_dir);
    mutate(&mut config);
    let json = serde_json::to_string_pretty(&config).map_err(io::Error::other)?;
    fs::write(config_dir.join("config.json"), json)
}


#[cfg(test)]
mod tests;
