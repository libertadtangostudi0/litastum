use crossterm::event::{KeyCode, KeyEvent};

use crate::config;


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


#[cfg(test)]
mod tests {
    use super::*;

    fn menu_with(themes: Vec<&str>) -> ThemeMenu {
        ThemeMenu { themes: themes.into_iter().map(String::from).collect(), selected: 0 }
    }

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

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
    }

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
