use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use tracing::debug;

use crate::app::{App, Mode};
use super::config;


/// State for the F9 "pick a color scheme" popup — a minimal analog of
/// Far Manager's F9 menu, scoped to just color schemes for now (a real
/// top-menu bar — Left/Files/Commands/Options/... — is a much bigger
/// feature; see `TODO.md`).
pub struct ThemeMenu {
    /// Filename stems of every `themes/*.json` file found in the
    /// config dir at the moment F9 was pressed — a snapshot, not
    /// live-refreshed while the menu stays open.
    pub themes: Vec<String>,
    pub selected: usize,
}


impl ThemeMenu {
    /// Scans `<config_dir>/themes/` for theme files. Empty (not an
    /// error) if there's no config dir or no `themes/` subdirectory yet
    /// — same "nothing configured" case `config.rs` already treats as
    /// normal, not a failure to report.
    pub fn open() -> Self {
        Self { themes: config::list_theme_names(), selected: 0 }
    }


    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }


    pub fn move_down(&mut self) {
        if self.selected + 1 < self.themes.len() {
            self.selected += 1;
        }
    }


    pub fn selected_theme(&self) -> Option<&str> {
        self.themes.get(self.selected).map(String::as_str)
    }
}


/// A user-triggered action on the theme-picker popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMenuCommand {
    Up,
    Down,
    /// Apply the highlighted theme as both the interface and editor
    /// theme — the common case (one scheme driving everything).
    ApplyBoth,
    /// Apply as the interface theme only, leaving `editor_theme`
    /// untouched — for anyone who wants the two independent, matching
    /// how Far Manager itself keeps them (see
    /// `.claude/rules/litastum-theming.md`).
    ApplyInterfaceOnly,
    ApplyEditorOnly,
    Close,
    Ignore,
}


/// Resolves a raw key press on the theme-picker popup.
pub fn resolve(key: KeyEvent) -> ThemeMenuCommand {
    match key.code {
        KeyCode::Up => ThemeMenuCommand::Up,
        KeyCode::Down => ThemeMenuCommand::Down,
        KeyCode::Enter => ThemeMenuCommand::ApplyBoth,
        KeyCode::Char('i' | 'I') => ThemeMenuCommand::ApplyInterfaceOnly,
        KeyCode::Char('e' | 'E') => ThemeMenuCommand::ApplyEditorOnly,
        KeyCode::Esc => ThemeMenuCommand::Close,
        _ => ThemeMenuCommand::Ignore,
    }
}


/// Key handling on the F9 color-scheme picker: `Enter` applies the
/// highlighted theme as both interface and editor theme, `I`/`E` apply
/// just one side, `Esc` closes without changing anything. Applying
/// updates `app.theme`/`app.syntax_theme` immediately — no restart —
/// and persists the choice to `config.json` on a best-effort basis
/// (`config.rs` logs and carries on if that write fails; the live
/// preview still applies). Moved here from `main.rs` so this module
/// owns its own state (`ThemeMenu`) *and* handling.
pub fn handle_theme_menu_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Mode::ThemeMenu(menu) = &mut app.mode else {
        return Ok(());
    };

    let command = resolve(key);
    debug!(?key, ?command, "theme menu key");

    match command {
        ThemeMenuCommand::Up => menu.move_up(),
        ThemeMenuCommand::Down => menu.move_down(),
        ThemeMenuCommand::Close => app.mode = Mode::Browsing,
        ThemeMenuCommand::ApplyBoth | ThemeMenuCommand::ApplyInterfaceOnly | ThemeMenuCommand::ApplyEditorOnly => {
            let Some(name) = menu.selected_theme().map(str::to_string) else {
                return Ok(());
            };
            if matches!(command, ThemeMenuCommand::ApplyBoth | ThemeMenuCommand::ApplyInterfaceOnly) {
                if let Some(theme) = config::set_interface_theme(&name) {
                    app.theme = theme;
                }
            }
            if matches!(command, ThemeMenuCommand::ApplyBoth | ThemeMenuCommand::ApplyEditorOnly) {
                if let Some(syntax_theme) = config::set_editor_theme(&name) {
                    app.syntax_theme = Some(syntax_theme);
                }
            }
            app.mode = Mode::Browsing;
        }
        ThemeMenuCommand::Ignore => {}
    }

    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::key;

    fn menu_with(themes: Vec<&str>) -> ThemeMenu {
        ThemeMenu { themes: themes.into_iter().map(String::from).collect(), selected: 0 }
    }

    mod theme_menu_state_tests {
        use super::*;

    #[test]
    fn move_down_clamped_at_last_theme() {
        let mut menu = menu_with(vec!["a", "b"]);
        menu.move_down();
        assert_eq!(menu.selected, 1);
        menu.move_down();
        assert_eq!(menu.selected, 1);
    }

    #[test]
    fn move_up_clamped_at_first_theme() {
        let mut menu = menu_with(vec!["a", "b"]);
        menu.move_up();
        assert_eq!(menu.selected, 0);
    }

    #[test]
    fn selected_theme_is_none_when_list_is_empty() {
        let menu = menu_with(vec![]);
        assert_eq!(menu.selected_theme(), None);
    }

    #[test]
    fn selected_theme_tracks_the_cursor() {
        let mut menu = menu_with(vec!["a", "b", "c"]);
        menu.move_down();
        assert_eq!(menu.selected_theme(), Some("b"));
    }
    }

    mod resolve_tests {
        use super::*;

    #[test]
    fn enter_applies_both() {
        assert_eq!(resolve(key(KeyCode::Enter)), ThemeMenuCommand::ApplyBoth);
    }

    #[test]
    fn i_and_e_apply_one_side_each() {
        assert_eq!(resolve(key(KeyCode::Char('i'))), ThemeMenuCommand::ApplyInterfaceOnly);
        assert_eq!(resolve(key(KeyCode::Char('E'))), ThemeMenuCommand::ApplyEditorOnly);
    }

    #[test]
    fn esc_closes() {
        assert_eq!(resolve(key(KeyCode::Esc)), ThemeMenuCommand::Close);
    }

    #[test]
    fn unbound_key_is_ignored() {
        assert_eq!(resolve(key(KeyCode::Char('z'))), ThemeMenuCommand::Ignore);
    }
    }

    mod handle_theme_menu_key_tests {
        use super::*;

    /// A real `App` (no terminal needed) in `Mode::ThemeMenu`, for
    /// exercising `handle_theme_menu_key` end to end.
    ///
    /// Deliberately does *not* cover the `Enter`/`I`/`E` apply branches
    /// with a non-empty theme list: `config::set_interface_theme`/
    /// `set_editor_theme` write to the *real* OS config directory
    /// (`config_dir()`, not an injectable path like `try_persist`'s own
    /// tests use) — calling them here would mutate the actual user's
    /// `config.json` as a side effect of running the test suite, which
    /// is worse than not testing that path at all. `config.rs`'s own
    /// tests have the same limitation, for the same reason.
    fn app_in_theme_menu(themes: Vec<&str>) -> App {
        let mut app = crate::test_support::test_app(crate::test_support::unique_scratch_dir("theme-menu"));
        app.mode = Mode::ThemeMenu(menu_with(themes));
        app
    }

    #[test]
    fn handle_theme_menu_key_up_and_down_move_the_cursor() {
        let mut app = app_in_theme_menu(vec!["a", "b", "c"]);

        handle_theme_menu_key(&mut app, key(KeyCode::Down)).unwrap();

        let Mode::ThemeMenu(menu) = &app.mode else { panic!("expected Mode::ThemeMenu") };
        assert_eq!(menu.selected, 1);
    }

    #[test]
    fn handle_theme_menu_key_esc_closes_to_browsing() {
        let mut app = app_in_theme_menu(vec!["a"]);

        handle_theme_menu_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn handle_theme_menu_key_ignores_unrelated_keys_and_stays_open() {
        let mut app = app_in_theme_menu(vec!["a"]);

        handle_theme_menu_key(&mut app, key(KeyCode::Char('z'))).unwrap();

        assert!(matches!(app.mode, Mode::ThemeMenu(_)));
    }

    #[test]
    fn handle_theme_menu_key_enter_with_no_themes_is_a_noop() {
        let mut app = app_in_theme_menu(vec![]);

        handle_theme_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

        // Guarded by ThemeMenu::selected_theme() returning None before
        // config::set_interface_theme is ever reached -- safe to test
        // without touching the real config dir.
        assert!(matches!(app.mode, Mode::ThemeMenu(_)), "nothing to apply, should stay open");
    }

    #[test]
    fn handle_theme_menu_key_is_a_noop_outside_theme_menu_mode() {
        let mut app = app_in_theme_menu(vec!["a"]);
        app.mode = Mode::Browsing;

        handle_theme_menu_key(&mut app, key(KeyCode::Down)).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
    }
    }
}
