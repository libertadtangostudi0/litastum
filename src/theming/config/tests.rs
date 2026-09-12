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
    fn popup_style_round_trips_through_try_persist_and_read_config() {
        let dir = scratch_dir();
        try_persist(&dir, |c| c.popup_style = Some(PopupStyle::Classic)).unwrap();

        let config = read_config(&dir);
        assert_eq!(config.popup_style, Some(PopupStyle::Classic));
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

    /// The classic (non-default) "GitHub Dark" variant is still bundled
    /// and selectable, just not the code-level default -- see
    /// `default_theme`'s own doc comment.
    #[test]
    fn find_scheme_finds_the_bundled_classic_github_dark_theme() {
        let scheme = find_scheme("github-dark").expect("github-dark.json should parse");
        assert_eq!(scheme.name, "GitHub Dark");
    }

    /// The bundled default theme (`themes/github-dark-default.json`)
    /// must actually be found via the same cwd fallback --
    /// `default_theme` silently falls back to the hardcoded built-in if
    /// this file is ever missing or renamed, so a broken bundling
    /// wouldn't otherwise show up as a loud failure anywhere.
    #[test]
    fn find_scheme_finds_the_bundled_default_github_dark_default_theme() {
        let scheme = find_scheme("github-dark-default").expect("github-dark-default.json should parse");
        assert_eq!(scheme.name, "GitHub Dark Default");
    }

    #[test]
    fn default_theme_resolves_both_halves_from_the_bundled_github_dark_default_file() {
        let (_theme, syntax_theme) = default_theme();
        assert!(syntax_theme.is_some(), "the bundled github-dark-default.json should also drive the editor's syntax theme, not just the interface");
    }
}
