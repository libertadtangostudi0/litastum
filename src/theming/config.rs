use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use edtui::syntect::highlighting::Theme as SynTheme;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use super::scheme::ColorScheme;
use super::theme::Theme;


/// This app's config directory (`<OS config dir>/litastum/`), if the
/// platform gives us one at all (some CI/headless environments don't
/// report a home directory — that's not fatal, it just means no custom
/// theme is possible, same as if the directory were simply empty).
fn config_dir() -> Option<PathBuf> {
    ProjectDirs::from("", "", "litastum").map(|dirs| dirs.config_dir().to_path_buf())
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
fn find_scheme(name: &str) -> Option<ColorScheme> {
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
        return (Theme::dark(), None);
    };

    let config = read_config(&config_dir);

    let theme = config
        .interface_theme
        .as_deref()
        .and_then(find_scheme)
        .map(|scheme| scheme.to_theme())
        .unwrap_or_else(Theme::dark);

    let syntax_theme = config
        .editor_theme
        .as_deref()
        .and_then(find_scheme)
        .map(|scheme| scheme.to_syntax_theme());

    (theme, syntax_theme)
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
mod tests {
    use super::*;
    use crate::test_support::unique_scratch_dir;

    /// A distinct scratch directory per test — never the user's real
    /// config dir, since `config_dir: &Path` is a plain parameter on
    /// both functions under test here.
    fn scratch_dir() -> PathBuf {
        unique_scratch_dir("config")
    }

    mod read_config_tests {
        use super::*;

    #[test]
    fn read_config_falls_back_to_default_when_file_is_missing() {
        let dir = scratch_dir();
        let config = read_config(&dir);
        assert_eq!(config.interface_theme, None);
        assert_eq!(config.editor_theme, None);
    }

    #[test]
    fn read_config_falls_back_to_default_on_malformed_json() {
        let dir = scratch_dir();
        fs::write(dir.join("config.json"), "{ not json").unwrap();
        let config = read_config(&dir);
        assert_eq!(config.interface_theme, None);
    }

    #[test]
    fn read_config_parses_both_keys_independently() {
        let dir = scratch_dir();
        fs::write(dir.join("config.json"), r#"{"interface_theme": "a", "editor_theme": "b"}"#).unwrap();
        let config = read_config(&dir);
        assert_eq!(config.interface_theme.as_deref(), Some("a"));
        assert_eq!(config.editor_theme.as_deref(), Some("b"));
    }

    #[test]
    fn read_config_accepts_just_one_key() {
        let dir = scratch_dir();
        fs::write(dir.join("config.json"), r#"{"interface_theme": "a"}"#).unwrap();
        let config = read_config(&dir);
        assert_eq!(config.interface_theme.as_deref(), Some("a"));
        assert_eq!(config.editor_theme, None);
    }
    }

    mod load_scheme_tests {
        use super::*;

    const VALID_SCHEME_JSON: &str = r##"{
        "name": "Test", "black": "#000000", "red": "#ff0000", "green": "#00ff00",
        "yellow": "#ffff00", "blue": "#0000ff", "purple": "#ff00ff", "cyan": "#00ffff",
        "white": "#ffffff", "brightBlack": "#111111", "brightRed": "#ff1111",
        "brightGreen": "#11ff11", "brightYellow": "#ffff11", "brightBlue": "#1111ff",
        "brightPurple": "#ff11ff", "brightCyan": "#11ffff", "brightWhite": "#eeeeee",
        "background": "#000000", "foreground": "#ffffff",
        "selectionBackground": "#222222", "cursorColor": "#333333"
    }"##;

    #[test]
    fn load_scheme_returns_none_when_theme_file_is_missing() {
        let dir = scratch_dir();
        assert!(load_scheme(&dir, "nonexistent").is_none());
    }

    #[test]
    fn load_scheme_parses_a_valid_theme_file() {
        // load_scheme takes the themes directory itself now (the
        // "themes" path segment is added by theme_search_dirs, not
        // load_scheme) -- no subdirectory needed here.
        let dir = scratch_dir();
        fs::write(dir.join("mine.json"), VALID_SCHEME_JSON).unwrap();

        let scheme = load_scheme(&dir, "mine").expect("theme should load");
        assert_eq!(scheme.background, "#000000");
    }

    #[test]
    fn a_broken_editor_theme_does_not_affect_the_interface_theme() {
        // Regression coverage for the actual point of splitting the two
        // keys: one broken half must not take the other down with it.
        let dir = scratch_dir();
        fs::write(dir.join("good.json"), VALID_SCHEME_JSON).unwrap();

        let interface = load_scheme(&dir, "good");
        let editor = load_scheme(&dir, "does-not-exist");

        assert!(interface.is_some(), "interface theme should still load");
        assert!(editor.is_none(), "missing editor theme file should just be None, not a panic");
    }
    }

    mod persist_tests {
        use super::*;

    #[test]
    fn try_persist_writes_a_fresh_config_when_none_existed() {
        let dir = scratch_dir();
        try_persist(&dir, |c| c.interface_theme = Some("mine".to_string())).unwrap();

        let config = read_config(&dir);
        assert_eq!(config.interface_theme.as_deref(), Some("mine"));
        assert_eq!(config.editor_theme, None);
    }

    #[test]
    fn try_persist_merges_into_an_existing_config_without_clobbering_the_other_key() {
        let dir = scratch_dir();
        fs::write(dir.join("config.json"), r#"{"editor_theme": "keep-me"}"#).unwrap();

        try_persist(&dir, |c| c.interface_theme = Some("new".to_string())).unwrap();

        let config = read_config(&dir);
        assert_eq!(config.interface_theme.as_deref(), Some("new"));
        assert_eq!(config.editor_theme.as_deref(), Some("keep-me"), "unrelated key must survive the write");
    }

    /// `save_setup`/`load_active_shell` themselves aren't tested
    /// directly — like `set_interface_theme`/`set_editor_theme`, they
    /// go through the real OS `config_dir()`, not an injectable path,
    /// so exercising them here would mutate the actual user's
    /// `config.json` as a side effect of running the test suite (same
    /// limitation, same reasoning, as `theme_menu.rs`'s tests). This
    /// pins down the `active_shell` round trip through the
    /// injectable-path half both of them are thin wrappers around.
    #[test]
    fn active_shell_round_trips_through_try_persist_and_read_config() {
        let dir = scratch_dir();
        try_persist(&dir, |c| c.active_shell = Some("PowerShell".to_string())).unwrap();

        let config = read_config(&dir);
        assert_eq!(config.active_shell.as_deref(), Some("PowerShell"));
    }

    #[test]
    fn saving_the_active_shell_does_not_clobber_an_existing_theme_choice() {
        let dir = scratch_dir();
        fs::write(dir.join("config.json"), r#"{"interface_theme": "keep-me"}"#).unwrap();

        try_persist(&dir, |c| c.active_shell = Some("Command Prompt".to_string())).unwrap();

        let config = read_config(&dir);
        assert_eq!(config.interface_theme.as_deref(), Some("keep-me"));
        assert_eq!(config.active_shell.as_deref(), Some("Command Prompt"));
    }
    }

    mod theme_discovery_tests {
        use super::*;

    // Regression coverage for a real bug: theme lookup only checked the
    // OS config dir, so the repo's own bundled `themes/apple-system-
    // colors.json` -- visible right there in the file panel when
    // running `cargo run` from the repo root -- never showed up in the
    // F9 picker. `cargo test`'s working directory is the package root,
    // same as `cargo run`'s, so these exercise the exact same path.

    #[test]
    fn list_theme_names_finds_the_repo_bundled_example_via_cwd_fallback() {
        let names = list_theme_names();
        assert!(names.contains(&"apple-system-colors".to_string()), "names: {names:?}");
    }

    #[test]
    fn find_scheme_finds_the_repo_bundled_example_via_cwd_fallback() {
        assert!(find_scheme("apple-system-colors").is_some());
    }

    #[test]
    fn find_scheme_finds_the_second_bundled_example_too() {
        let scheme = find_scheme("alien-blood").expect("alien-blood.json should parse");
        assert_eq!(scheme.name, "AlienBlood");
    }
    }
}
