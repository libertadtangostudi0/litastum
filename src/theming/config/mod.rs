use std::fs;
use std::io;
use std::path::{Path, PathBuf};

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


/// This app's config directory -- `appdata/` in the project
/// (`crate::app_data`), which becomes `%APPDATA%\litastum\` once there's
/// an installer. `None` in a test build, so no test depends on or writes
/// real config. History: docs/history/theming.md.
pub(crate) fn config_dir() -> Option<PathBuf> {
    crate::app_data::app_data_dir()
}


/// Directories searched for theme files, in order: the config dir's
/// `themes/` (the user's own), then the bundled set
/// (`bundled_themes_dirs`). `config.json` itself is only ever read from
/// the config dir.
fn theme_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = config_dir() {
        dirs.push(dir.join("themes"));
    }
    dirs.extend(bundled_themes_dirs(std::env::current_exe().ok().as_deref()));
    dirs
}


/// Where the bundled themes are: `themes/` next to the executable (a
/// `cargo xtask dist` layout), then the project's own `themes/`. Never
/// relative to the current directory: that used to be the only place,
/// and litastum started anywhere else -- as its own window does, in the
/// directory it was opened from -- found no themes at all, not even the
/// default one. History: docs/history/theming.md.
fn bundled_themes_dirs(exe: Option<&std::path::Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = exe.and_then(std::path::Path::parent) {
        dirs.push(dir.join("themes"));
    }
    dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("themes"));
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
    /// litastum's window (`gui/`) keeps each screen's zoom here, by the
    /// screen it reports (`main`, `compare`, `conflict`). Unused by the
    /// console app, but kept: every save writes the whole file, and an
    /// unknown key would be dropped.
    window_zoom: Option<std::collections::BTreeMap<String, f32>>,
    /// The panels' last directories (`last_paths`), opened again at the
    /// next start (`load_panel_paths`).
    left_panel_path: Option<PathBuf>,
    right_panel_path: Option<PathBuf>,
    /// Windows Terminal's zoom per screen, in its steps (`terminal_zoom`).
    terminal_zoom: Option<std::collections::BTreeMap<String, i32>>,
    /// Where the terminal's tab is now, in steps.
    terminal_zoom_now: Option<TerminalZoomNow>,
}


/// Windows Terminal's tab (`WT_SESSION`) and the steps it's zoomed by
/// now: a litastum that ended without putting it back left it there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalZoomNow {
    pub session: String,
    pub steps: i32,
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


/// The UI choices restored at startup: the popup style (F9 -> Options ->
/// UI), the keymap new editor sessions open with, and Compare's line-
/// ending display. Kept on `App::settings`; each menu changes its field
/// and saves the whole thing (`save_settings`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Settings {
    pub popup_style: PopupStyle,
    pub editor_keymap_mode: EditorKeymapMode,
    pub compare_line_ending_display: LineEndingDisplay,
}


/// `config.json`'s settings, each missing field falling back to its
/// default on its own -- or all defaults without a config directory.
pub fn load_settings() -> Settings {
    let Some(config_dir) = config_dir() else {
        debug!("no config directory available on this platform; using default settings");
        return Settings::default();
    };
    settings_from(&read_config(&config_dir))
}


fn settings_from(config: &Config) -> Settings {
    Settings {
        popup_style: config.popup_style.unwrap_or_default(),
        editor_keymap_mode: config.editor_keymap_mode.unwrap_or_default(),
        compare_line_ending_display: config.compare_line_ending_display.unwrap_or_default(),
    }
}


/// The left and right panels' last directories, if saved -- litastum
/// opens them again at start (requested), each one that still exists.
pub fn load_panel_paths() -> [Option<PathBuf>; 2] {
    let Some(config_dir) = config_dir() else {
        return [None, None];
    };
    let config = read_config(&config_dir);
    [config.left_panel_path, config.right_panel_path]
}


/// Windows Terminal's zoom per screen, in steps (`terminal_zoom`) -- none
/// saved, every screen at the terminal's own size -- and where its tab
/// was last.
pub fn load_terminal_zoom() -> (std::collections::BTreeMap<String, i32>, Option<TerminalZoomNow>) {
    let Some(config) = config_dir().map(|dir| read_config(&dir)) else {
        return Default::default();
    };
    (config.terminal_zoom.unwrap_or_default(), config.terminal_zoom_now)
}


/// Saves Windows Terminal's zoom per screen and where its tab is,
/// best-effort.
pub fn save_terminal_zoom(steps: &std::collections::BTreeMap<String, i32>, now: TerminalZoomNow) {
    if let Some(config_dir) = config_dir() {
        persist(&config_dir, |config| {
            config.terminal_zoom = Some(steps.clone());
            config.terminal_zoom_now = Some(now);
        });
    }
}


/// Saves the panels' directories, best-effort (`last_paths` decides
/// when).
pub fn save_panel_paths(left: &Path, right: &Path) {
    if let Some(config_dir) = config_dir() {
        persist(&config_dir, |config| {
            config.left_panel_path = Some(left.to_path_buf());
            config.right_panel_path = Some(right.to_path_buf());
        });
    }
}


/// Persists `settings`, best-effort -- a failed write is logged, and the
/// change still applies for this session.
pub fn save_settings(settings: &Settings) {
    if let Some(config_dir) = config_dir() {
        persist(&config_dir, |config| {
            config.popup_style = Some(settings.popup_style);
            config.editor_keymap_mode = Some(settings.editor_keymap_mode);
            config.compare_line_ending_display = Some(settings.compare_line_ending_display);
        });
    }
}


/// Reads the current `config.json` (or defaults, if there isn't one
/// yet), applies `mutate`, and writes it back. Logs and gives up
/// quietly on any I/O/serialization failure — never panics, and the
/// caller applies the theme live regardless of whether this succeeded.
fn persist(config_dir: &Path, mutate: impl FnOnce(&mut Config)) {
    if let Err(err) = try_persist(config_dir, mutate) {
        warn!(%err, "failed to save config.json (the change still applies for this session)");
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
