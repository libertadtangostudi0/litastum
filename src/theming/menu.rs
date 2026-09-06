use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use tracing::debug;

use crate::app::{App, Mode};
use crate::command_line::CommandHistoryMenu;
use crate::explorer::FindFileState;
use super::config;
use super::theme_menu::ThemeMenu;

/// F9's top menu. Enough structure to reach what's actually been asked
/// for so far (Commands → Find file/History, Options → Color schemes/
/// Save setup) — not Far Manager's full Left/Files/Commands/Options/
/// View/Right top-menu bar; see `TODO.md` for what a real one would
/// still need.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuLevel {
    Main,
    Commands,
    Options,
}


impl MenuLevel {
    /// The items shown at this level, in order — `MainMenu::selected`
    /// indexes into this.
    pub fn items(self) -> &'static [&'static str] {
        match self {
            MenuLevel::Main => &["Commands", "Options"],
            MenuLevel::Commands => &["Find file", "History"],
            MenuLevel::Options => &["Color schemes", "Save setup"],
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


    /// Descends into `level`, resetting the cursor.
    pub fn enter(&mut self, level: MenuLevel) {
        self.level = level;
        self.selected = 0;
    }


    /// Backs up one level. Returns `true` if it moved up a level (the
    /// caller stays in the menu); `false` if already at the top level
    /// (the caller should close the menu entirely). Every non-`Main`
    /// level's parent is `Main` — fine while the menu stays two levels
    /// deep; would need each level to know its own parent if a third
    /// level is ever added.
    pub fn back(&mut self) -> bool {
        match self.level {
            MenuLevel::Commands | MenuLevel::Options => {
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


/// Key handling on the F9 top menu: `Up`/`Down` move, `Enter` descends
/// into a submenu or, at a leaf item, runs whatever that item does;
/// `Esc` backs up one level, or closes the menu entirely if already at
/// the top. Moved here from `main.rs` so this module owns its own state
/// (`MainMenu`) *and* handling, the same way `theme_menu.rs` does for
/// its own picker.
pub fn handle_main_menu_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Mode::MainMenu(menu_state) = &mut app.mode else {
        return Ok(());
    };

    let command = resolve(key);
    debug!(?key, ?command, "main menu key");

    match command {
        MenuCommand::Up => menu_state.move_up(),
        MenuCommand::Down => menu_state.move_down(),
        MenuCommand::Back => {
            if !menu_state.back() {
                app.mode = Mode::Browsing;
            }
        }
        // Matched on (level, item label) rather than a positional
        // index, so adding/reordering an item in `MenuLevel::items`
        // can't silently wire Select up to the wrong action.
        MenuCommand::Select => {
            let level = menu_state.level;
            let item = level.items().get(menu_state.selected).copied();
            match (level, item) {
                (MenuLevel::Main, Some("Commands")) => menu_state.enter(MenuLevel::Commands),
                (MenuLevel::Main, Some("Options")) => menu_state.enter(MenuLevel::Options),
                (MenuLevel::Commands, Some("Find file")) => app.mode = Mode::FindFile(FindFileState::new()),
                (MenuLevel::Commands, Some("History")) => app.mode = Mode::CommandHistory(CommandHistoryMenu::open()),
                (MenuLevel::Options, Some("Color schemes")) => app.mode = Mode::ThemeMenu(ThemeMenu::open()),
                (MenuLevel::Options, Some("Save setup")) => {
                    // Far Manager's own Shift+F9 -- persists the
                    // current session's choices (so far, just which
                    // shell profile is active) rather than every
                    // choice auto-persisting the moment it's made, the
                    // way the theme picker's own choices already do.
                    let name = app.shell_profiles[app.active_shell].name.clone();
                    config::save_setup(&name);
                    debug!(shell = name, "save setup: persisted active shell profile");
                    app.mode = Mode::Browsing;
                }
                _ => {}
            }
        }
        MenuCommand::Ignore => {}
    }

    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{key, unique_scratch_dir};

    mod main_menu_state_tests {
        use super::*;

    #[test]
    fn move_down_steps_through_main_level_items() {
        let mut menu = MainMenu::open(); // Main level: ["Commands", "Options"]
        menu.move_down();
        assert_eq!(menu.selected, 1);
        menu.move_down();
        assert_eq!(menu.selected, 1, "clamped at the last item");
    }

    #[test]
    fn move_up_clamped_at_first_item() {
        let mut menu = MainMenu::open();
        menu.move_up();
        assert_eq!(menu.selected, 0);
    }

    #[test]
    fn enter_switches_level_and_resets_cursor() {
        let mut menu = MainMenu::open();
        menu.selected = 1;
        menu.enter(MenuLevel::Options);
        assert_eq!(menu.level, MenuLevel::Options);
        assert_eq!(menu.selected, 0);
        assert_eq!(menu.level.items(), &["Color schemes", "Save setup"]);
    }

    #[test]
    fn back_from_options_returns_to_main() {
        let mut menu = MainMenu::open();
        menu.enter(MenuLevel::Options);

        let stayed_in_menu = menu.back();

        assert!(stayed_in_menu);
        assert_eq!(menu.level, MenuLevel::Main);
    }

    #[test]
    fn back_from_commands_returns_to_main() {
        let mut menu = MainMenu::open();
        menu.enter(MenuLevel::Commands);

        let stayed_in_menu = menu.back();

        assert!(stayed_in_menu);
        assert_eq!(menu.level, MenuLevel::Main);
    }

    #[test]
    fn back_from_main_signals_close() {
        let mut menu = MainMenu::open();
        assert!(!menu.back());
    }
    }

    mod resolve_tests {
        use super::*;

    #[test]
    fn resolve_maps_keys() {
        assert_eq!(resolve(key(KeyCode::Up)), MenuCommand::Up);
        assert_eq!(resolve(key(KeyCode::Down)), MenuCommand::Down);
        assert_eq!(resolve(key(KeyCode::Enter)), MenuCommand::Select);
        assert_eq!(resolve(key(KeyCode::Esc)), MenuCommand::Back);
        assert_eq!(resolve(key(KeyCode::Char('z'))), MenuCommand::Ignore);
    }
    }

    mod handle_main_menu_key_tests {
        use super::*;

    /// A real `App` (no terminal needed — `App::new` just wants a
    /// directory) in `Mode::MainMenu`, for exercising
    /// `handle_main_menu_key` end to end rather than just `MainMenu`'s
    /// own methods.
    fn app_in_main_menu() -> App {
        let mut app = crate::test_support::test_app(unique_scratch_dir("menu"));
        app.mode = Mode::MainMenu(MainMenu::open());
        app
    }

    #[test]
    fn handle_main_menu_key_select_at_main_level_enters_commands() {
        let mut app = app_in_main_menu();

        handle_main_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

        let Mode::MainMenu(menu) = &app.mode else { panic!("expected Mode::MainMenu") };
        assert_eq!(menu.level, MenuLevel::Commands, "first item at Main level");
    }

    #[test]
    fn handle_main_menu_key_select_at_main_level_second_item_enters_options() {
        let mut app = app_in_main_menu();
        let Mode::MainMenu(menu) = &mut app.mode else { unreachable!() };
        menu.move_down();

        handle_main_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

        let Mode::MainMenu(menu) = &app.mode else { panic!("expected Mode::MainMenu") };
        assert_eq!(menu.level, MenuLevel::Options);
    }

    #[test]
    fn handle_main_menu_key_select_color_schemes_opens_theme_menu() {
        let mut app = app_in_main_menu();
        let Mode::MainMenu(menu) = &mut app.mode else { unreachable!() };
        menu.enter(MenuLevel::Options); // ["Color schemes", "Save setup"]

        handle_main_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert!(matches!(app.mode, Mode::ThemeMenu(_)));
    }

    #[test]
    fn handle_main_menu_key_select_save_setup_closes_the_menu() {
        // Doesn't assert the config.json write itself happened --
        // config::save_setup goes through the real OS config dir, same
        // reason config.rs's own tests don't exercise it directly (see
        // that module). This only pins down the app-visible effect:
        // the action runs (doesn't panic) and the menu closes.
        let mut app = app_in_main_menu();
        let Mode::MainMenu(menu) = &mut app.mode else { unreachable!() };
        menu.enter(MenuLevel::Options);
        menu.move_down(); // "Save setup"

        handle_main_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn handle_main_menu_key_select_find_file_opens_find_file_mode() {
        let mut app = app_in_main_menu();
        let Mode::MainMenu(menu) = &mut app.mode else { unreachable!() };
        menu.enter(MenuLevel::Commands); // ["Find file", "History"]

        handle_main_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert!(matches!(app.mode, Mode::FindFile(_)));
    }

    #[test]
    fn handle_main_menu_key_select_history_opens_command_history_mode() {
        let mut app = app_in_main_menu();
        let Mode::MainMenu(menu) = &mut app.mode else { unreachable!() };
        menu.enter(MenuLevel::Commands);
        menu.move_down(); // "History"

        handle_main_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert!(matches!(app.mode, Mode::CommandHistory(_)));
    }

    #[test]
    fn handle_main_menu_key_back_at_options_level_returns_to_main() {
        let mut app = app_in_main_menu();
        let Mode::MainMenu(menu) = &mut app.mode else { unreachable!() };
        menu.enter(MenuLevel::Options);

        handle_main_menu_key(&mut app, key(KeyCode::Esc)).unwrap();

        let Mode::MainMenu(menu) = &app.mode else { panic!("expected Mode::MainMenu") };
        assert_eq!(menu.level, MenuLevel::Main, "should back up a level, not close");
    }

    #[test]
    fn handle_main_menu_key_back_at_main_level_closes_the_menu() {
        let mut app = app_in_main_menu();

        handle_main_menu_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn handle_main_menu_key_is_a_noop_outside_main_menu_mode() {
        let mut app = app_in_main_menu();
        app.mode = Mode::Browsing;

        handle_main_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
    }
    }
}
