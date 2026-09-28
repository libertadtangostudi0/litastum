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
use crate::compare::LineEndingDisplay;
use crate::editor::EditorKeymapMode;

mod limits;
pub use limits::limits;


/// Overrides `config_dir()` with any directory -- for local development
/// (e.g. testing the F2 menu's common-directory fallback without files
/// under `%APPDATA%`).
// Only read from `#[cfg(not(test))]` code, hence the test-build allow.
#[cfg_attr(test, allow(dead_code))]
pub(crate) const CONFIG_DIR_ENV_VAR: &str = "LITASTUM_CONFIG_DIR";

/// This app's config directory: `LITASTUM_CONFIG_DIR` if set, else
/// `<OS config dir>/litastum/`, or `None` if the platform has none (not
/// fatal -- just nothing custom).
///
/// **Always `None` in a test build**, so no test depends on a
/// developer's real config (one did, once `LITASTUM_CONFIG_DIR` pointed
/// at a real menu). Gated here, at the one choke point. History: docs/history/theming.md.
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


/// Directories searched for theme files, in order: the config dir's
/// `themes/`, then `./themes/` (so a theme next to the binary or the
/// repo shows up in the picker without copying). `config.json` itself is
/// only ever read from the config dir.
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


/// litastum's `config.json` (JSON because `serde_json` is already needed
/// for theme files). Every field is optional and falls back on its own.
/// `interface_theme`/`editor_theme` are independent, as in Far; each
/// names a theme file stem in `themes/`.
#[derive(Debug, Deserialize, Serialize, Default)]
struct Config {
    interface_theme: Option<String>,
    editor_theme: Option<String>,
    /// Shell profile name saved by F9 -> Options -> Save setup (on
    /// demand, like Far's `Shift+F9`); applied at startup if it still
    /// exists.
    active_shell: Option<String>,
    /// F9 -> Options -> UI (`PopupStyle`), stored as the enum itself.
    popup_style: Option<PopupStyle>,
    /// The editor's F9 -> Keybindings (`EditorKeymapMode`).
    editor_keymap_mode: Option<EditorKeymapMode>,
    /// Compare's F9 -> Line endings (`LineEndingDisplay`).
    compare_line_ending_display: Option<LineEndingDisplay>,
    /// The fields below override `Limits` (`limits.rs`) one by one; no
    /// menu for them, hand-edit `config.json`.
    max_command_history: Option<usize>,
    find_file_max_results: Option<usize>,
    find_file_max_visited: Option<usize>,
    panel_min_column_width: Option<u16>,
    markdown_preview_page_size: Option<usize>,
    max_log_bytes: Option<u64>,
    max_paste_undo_stack: Option<usize>,
}


/// Loads the interface theme and, independently, the editor's syntax
/// theme. Never fails: anything missing or broken falls back to the
/// default for that half only, logged.
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


/// The default for both halves: bundled `github-dark-default` (what VS
/// Code's `GitHub Dark Default` is generated from -- not `github-dark`,
/// a different palette). Falls back to the hardcoded `Theme::dark()` /
/// `SYNTAX_THEME` if that file is missing or broken. History: docs/history/theming.md.
fn default_theme() -> (Theme, Option<SynTheme>) {
    match find_scheme("github-dark-default") {
        Some(scheme) => (scheme.to_theme(), Some(scheme.to_syntax_theme())),
        None => {
            warn!("bundled default theme \"github-dark-default\" not found; falling back to the hardcoded built-in theme");
            (Theme::dark(), None)
        }
    }
}


/// The configured theme names, unresolved -- the F9 picker marks the
/// current entries with them.
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


/// Reads and parses `<dir>/themes/<name>.json`, `None` on failure. A
/// missing file is `debug!` (several directories are searched); a broken
/// one is `warn!`.
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


/// Theme names (file stems) across `theme_search_dirs()`, deduplicated
/// and sorted, for the F9 picker.
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


/// Persists `name` as the interface theme and returns it for live use.
/// A failed write is only logged -- the live preview still applies.
/// `None` if the theme file vanished since the picker listed it.
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


/// F9 -> Options -> Save setup (Far's `Shift+F9`): persists the active
/// shell profile name, best-effort.
pub fn save_setup(shell_profile_name: &str) {
    if let Some(config_dir) = config_dir() {
        persist(&config_dir, |config| config.active_shell = Some(shell_profile_name.to_string()));
    }
}


/// The shell profile name saved by `save_setup`; `main.rs` applies it
/// if a profile by that name still exists.
pub fn load_active_shell() -> Option<String> {
    let config_dir = config_dir()?;
    read_config(&config_dir).active_shell
}


/// The configured popup style, or the default.
pub fn load_active_popup_style() -> PopupStyle {
    let Some(config_dir) = config_dir() else {
        debug!("no config directory available on this platform; using default popup style");
        return PopupStyle::default();
    };
    read_config(&config_dir).popup_style.unwrap_or_default()
}


/// Persists the popup style, best-effort.
pub fn set_popup_style(style: PopupStyle) {
    if let Some(config_dir) = config_dir() {
        persist(&config_dir, |config| config.popup_style = Some(style));
    }
}


/// The configured editor keymap, or the default (`Standard`).
pub fn load_active_editor_keymap_mode() -> EditorKeymapMode {
    let Some(config_dir) = config_dir() else {
        debug!("no config directory available on this platform; using default editor keymap mode");
        return EditorKeymapMode::default();
    };
    read_config(&config_dir).editor_keymap_mode.unwrap_or_default()
}


/// Persists the editor keymap new sessions open with, best-effort.
pub fn set_editor_keymap_mode(mode: EditorKeymapMode) {
    if let Some(config_dir) = config_dir() {
        persist(&config_dir, |config| config.editor_keymap_mode = Some(mode));
    }
}


/// The configured Compare line-ending display, or the default.
pub fn load_active_compare_line_ending_display() -> LineEndingDisplay {
    let Some(config_dir) = config_dir() else {
        debug!("no config directory available on this platform; using default line-ending display");
        return LineEndingDisplay::default();
    };
    read_config(&config_dir).compare_line_ending_display.unwrap_or_default()
}


/// Persists Compare's line-ending display, best-effort.
pub fn set_compare_line_ending_display(display: LineEndingDisplay) {
    if let Some(config_dir) = config_dir() {
        persist(&config_dir, |config| config.compare_line_ending_display = Some(display));
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
