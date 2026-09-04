use crossterm::event::{KeyCode, KeyEvent};

/// F9's top menu. Currently just enough structure to reach the color
/// scheme picker through a "Settings" submenu (what was actually
/// asked for) — not Far Manager's full Left/Files/Commands/Options/
/// View/Right top-menu bar; see `TODO.md` for what a real one would
/// still need.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuLevel {
    Main,
    Settings,
}


impl MenuLevel {
    /// The items shown at this level, in order — `MainMenu::selected`
    /// indexes into this.
    pub fn items(self) -> &'static [&'static str] {
        match self {
            MenuLevel::Main => &["Settings"],
            MenuLevel::Settings => &["Color schemes"],
        }
    }
}


pub struct MainMenu {
    pub level: MenuLevel,
    pub selected: usize,
}


impl MainMenu {
    pub fn open() -> Self {
        Self { level: MenuLevel::Main, selected: 0 }
    }


    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }


    pub fn move_down(&mut self) {
        if self.selected + 1 < self.level.items().len() {
            self.selected += 1;
        }
    }


    /// Descends into the "Settings" submenu, resetting the cursor.
    pub fn enter_settings(&mut self) {
        self.level = MenuLevel::Settings;
        self.selected = 0;
    }


    /// Backs up one level. Returns `true` if it moved up a level (the
    /// caller stays in the menu); `false` if already at the top level
    /// (the caller should close the menu entirely).
    pub fn back(&mut self) -> bool {
        match self.level {
            MenuLevel::Settings => {
                self.level = MenuLevel::Main;
                self.selected = 0;
                true
            }
            MenuLevel::Main => false,
        }
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuCommand {
    Up,
    Down,
    Select,
    Back,
    Ignore,
}


pub fn resolve(key: KeyEvent) -> MenuCommand {
    match key.code {
        KeyCode::Up => MenuCommand::Up,
        KeyCode::Down => MenuCommand::Down,
        KeyCode::Enter => MenuCommand::Select,
        KeyCode::Esc => MenuCommand::Back,
        _ => MenuCommand::Ignore,
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_down_clamped_at_last_item() {
        let mut menu = MainMenu::open(); // Main level, one item: "Settings"
        menu.move_down();
        assert_eq!(menu.selected, 0, "only one item at Main level");
    }

    #[test]
    fn move_up_clamped_at_first_item() {
        let mut menu = MainMenu::open();
        menu.move_up();
        assert_eq!(menu.selected, 0);
    }

    #[test]
    fn enter_settings_switches_level_and_resets_cursor() {
        let mut menu = MainMenu::open();
        menu.selected = 0;
        menu.enter_settings();
        assert_eq!(menu.level, MenuLevel::Settings);
        assert_eq!(menu.selected, 0);
        assert_eq!(menu.level.items(), &["Color schemes"]);
    }

    #[test]
    fn back_from_settings_returns_to_main() {
        let mut menu = MainMenu::open();
        menu.enter_settings();

        let stayed_in_menu = menu.back();

        assert!(stayed_in_menu);
        assert_eq!(menu.level, MenuLevel::Main);
    }

    #[test]
    fn back_from_main_signals_close() {
        let mut menu = MainMenu::open();
        assert!(!menu.back());
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
    }

    #[test]
    fn resolve_maps_keys() {
        assert_eq!(resolve(key(KeyCode::Up)), MenuCommand::Up);
        assert_eq!(resolve(key(KeyCode::Down)), MenuCommand::Down);
        assert_eq!(resolve(key(KeyCode::Enter)), MenuCommand::Select);
        assert_eq!(resolve(key(KeyCode::Esc)), MenuCommand::Back);
        assert_eq!(resolve(key(KeyCode::Char('z'))), MenuCommand::Ignore);
    }
}
