use color_eyre::eyre::Result;
use crossterm::event::KeyEvent;
use tracing::debug;

use crate::app::{App, Overlay};
use crate::choice_menu::{ChoiceMenu, MenuOutcome};
use crate::command_line::CommandHistoryMenu;
use crate::command_line::Effect;
use crate::explorer::FindFileState;
use super::config;
use super::popup_style_menu::open_popup_style_menu;
use super::theme_menu::ThemeMenu;

/// F9's top menu. Enough structure to reach what's actually been asked
/// for so far (Commands → Find file/History, Options → Color schemes/
/// UI/Save setup) — not Far Manager's full Left/Files/Commands/Options/
/// View/Right top-menu bar; see `TODO/f9-menu.md` for what a real one would
/// still need.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuLevel {
    Main,
    Commands,
    Options,
}


/// An item of the F9 menu. Items are matched by variant, never by label,
/// so renaming or reordering one can't silently unwire it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainMenuItem {
    Commands,
    Options,
    FindFile,
    History,
    ColorSchemes,
    Ui,
    SaveSetup,
}


impl MainMenuItem {
    pub fn label(self) -> &'static str {
        match self {
            Self::Commands => "Commands",
            Self::Options => "Options",
            Self::FindFile => "Find file",
            Self::History => "History",
            Self::ColorSchemes => "Color schemes",
            Self::Ui => "UI",
            Self::SaveSetup => "Save setup",
        }
    }
}


impl MenuLevel {
    /// The items shown at this level, in order.
    pub fn items(self) -> &'static [MainMenuItem] {
        match self {
            MenuLevel::Main => &[MainMenuItem::Commands, MainMenuItem::Options],
            MenuLevel::Commands => &[MainMenuItem::FindFile, MainMenuItem::History],
            MenuLevel::Options => &[MainMenuItem::ColorSchemes, MainMenuItem::Ui, MainMenuItem::SaveSetup],
        }
    }


    pub fn title(self) -> &'static str {
        match self {
            MenuLevel::Main => " Menu ",
            MenuLevel::Commands => " Commands ",
            MenuLevel::Options => " Options ",
        }
    }
}


pub struct MainMenu {
    pub level: MenuLevel,
    /// The current level's items.
    pub list: ChoiceMenu<MainMenuItem>,
}


impl MainMenu {
    pub fn open() -> Self {
        Self { level: MenuLevel::Main, list: ChoiceMenu::new(MenuLevel::Main.items(), None) }
    }


    /// Descends into `level`, cursor on its first item.
    pub fn enter(&mut self, level: MenuLevel) {
        self.level = level;
        self.list = ChoiceMenu::new(level.items(), None);
    }


    /// Backs up one level. Returns `true` if it moved up a level (the
    /// caller stays in the menu); `false` if already at the top level
    /// (the caller should close the menu entirely). Every non-`Main`
    /// level's parent is `Main` — fine while the menu stays two levels
    /// deep.
    pub fn back(&mut self) -> bool {
        match self.level {
            MenuLevel::Commands | MenuLevel::Options => {
                self.enter(MenuLevel::Main);
                true
            }
            MenuLevel::Main => false,
        }
    }
}


/// Key handling on the F9 top menu: `Up`/`Down` move, `Enter` descends
/// into a submenu or runs a leaf item; `Esc` backs up one level, or
/// closes the menu from the top.
pub fn handle_main_menu_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    let Some(Overlay::MainMenu(menu)) = &mut app.overlay else {
        return Ok(Effect::None);
    };

    let outcome = menu.list.handle_key(key);
    debug!(?key, ?outcome, "main menu key");

    match outcome {
        MenuOutcome::Open => {}
        MenuOutcome::Closed => {
            if !menu.back() {
                app.overlay = None;
            }
        }
        MenuOutcome::Chosen(item) => match item {
            MainMenuItem::Commands => menu.enter(MenuLevel::Commands),
            MainMenuItem::Options => menu.enter(MenuLevel::Options),
            MainMenuItem::FindFile => app.overlay = Some(Overlay::FindFile(FindFileState::new())),
            MainMenuItem::History => app.overlay = Some(Overlay::CommandHistory(CommandHistoryMenu::open())),
            MainMenuItem::ColorSchemes => app.overlay = Some(Overlay::ThemeMenu(ThemeMenu::open())),
            MainMenuItem::Ui => app.overlay = Some(Overlay::PopupStyleMenu(open_popup_style_menu(app.settings.popup_style))),
            MainMenuItem::SaveSetup => {
                // Far Manager's own Shift+F9 -- persists the current
                // session's choices (so far, the active shell profile)
                // on demand, unlike the theme picker's auto-persist.
                let name = app.shell_profiles[app.active_shell].name.clone();
                config::save_setup(&name);
                debug!(shell = name, "save setup: persisted active shell profile");
                app.overlay = None;
            }
        },
    }

    Ok(Effect::None)
}


#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;

    use super::*;
    use crate::app::Mode;
    use crate::test_support::{key, unique_scratch_dir};

    mod main_menu_state_tests {
        use super::*;

        #[test]
        fn opens_at_the_main_level() {
            let menu = MainMenu::open();
            assert_eq!(menu.level, MenuLevel::Main);
            assert_eq!(menu.list.selected(), MainMenuItem::Commands);
        }

        #[test]
        fn enter_switches_level_and_resets_cursor() {
            let mut menu = MainMenu::open();
            menu.list.move_down();
            menu.enter(MenuLevel::Options);
            assert_eq!(menu.level, MenuLevel::Options);
            assert_eq!(menu.list.selected_index(), 0);
            assert_eq!(menu.list.options(), &[MainMenuItem::ColorSchemes, MainMenuItem::Ui, MainMenuItem::SaveSetup]);
        }

        #[test]
        fn back_from_options_returns_to_main() {
            let mut menu = MainMenu::open();
            menu.enter(MenuLevel::Options);

            assert!(menu.back());
            assert_eq!(menu.level, MenuLevel::Main);
        }

        #[test]
        fn back_from_commands_returns_to_main() {
            let mut menu = MainMenu::open();
            menu.enter(MenuLevel::Commands);

            assert!(menu.back());
            assert_eq!(menu.level, MenuLevel::Main);
        }

        #[test]
        fn back_from_main_signals_close() {
            let mut menu = MainMenu::open();
            assert!(!menu.back());
        }
    }

    mod handle_main_menu_key_tests {
        use super::*;

    /// A real `App` (no terminal needed — `App::new` just wants a
    /// directory) in `Overlay::MainMenu`, for exercising
    /// `handle_main_menu_key` end to end rather than just `MainMenu`'s
    /// own methods.
    fn app_in_main_menu() -> App {
        let mut app = crate::test_support::test_app(unique_scratch_dir("menu"));
        app.overlay = Some(Overlay::MainMenu(MainMenu::open()));
        app
    }

    #[test]
    fn handle_main_menu_key_select_at_main_level_enters_commands() {
        let mut app = app_in_main_menu();

        handle_main_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

        let Some(Overlay::MainMenu(menu)) = &app.overlay else { panic!("expected Overlay::MainMenu") };
        assert_eq!(menu.level, MenuLevel::Commands, "first item at Main level");
    }

    #[test]
    fn handle_main_menu_key_select_at_main_level_second_item_enters_options() {
        let mut app = app_in_main_menu();
        let Some(Overlay::MainMenu(menu)) = &mut app.overlay else { unreachable!() };
        menu.list.move_down();

        handle_main_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

        let Some(Overlay::MainMenu(menu)) = &app.overlay else { panic!("expected Overlay::MainMenu") };
        assert_eq!(menu.level, MenuLevel::Options);
    }

    #[test]
    fn handle_main_menu_key_select_color_schemes_opens_theme_menu() {
        let mut app = app_in_main_menu();
        let Some(Overlay::MainMenu(menu)) = &mut app.overlay else { unreachable!() };
        menu.enter(MenuLevel::Options); // Color schemes, UI, Save setup

        handle_main_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert!(matches!(app.overlay, Some(Overlay::ThemeMenu(_))));
    }

    #[test]
    fn handle_main_menu_key_select_save_setup_closes_the_menu() {
        // Doesn't assert the config.json write itself happened --
        // config::save_setup goes through the real config dir, same
        // reason config.rs's own tests don't exercise it directly (see
        // that module). This only pins down the app-visible effect:
        // the action runs (doesn't panic) and the menu closes.
        let mut app = app_in_main_menu();
        let Some(Overlay::MainMenu(menu)) = &mut app.overlay else { unreachable!() };
        menu.enter(MenuLevel::Options);
        menu.list.move_down();
        menu.list.move_down(); // "Save setup"

        handle_main_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert!(app.overlay.is_none() && matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn handle_main_menu_key_select_ui_opens_popup_style_menu() {
        let mut app = app_in_main_menu();
        let Some(Overlay::MainMenu(menu)) = &mut app.overlay else { unreachable!() };
        menu.enter(MenuLevel::Options);
        menu.list.move_down(); // "UI"

        handle_main_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert!(matches!(app.overlay, Some(Overlay::PopupStyleMenu(_))));
    }

    #[test]
    fn handle_main_menu_key_select_find_file_opens_find_file_mode() {
        let mut app = app_in_main_menu();
        let Some(Overlay::MainMenu(menu)) = &mut app.overlay else { unreachable!() };
        menu.enter(MenuLevel::Commands); // ["Find file", "History"]

        handle_main_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert!(matches!(app.overlay, Some(Overlay::FindFile(_))));
    }

    #[test]
    fn handle_main_menu_key_select_history_opens_command_history_mode() {
        let mut app = app_in_main_menu();
        let Some(Overlay::MainMenu(menu)) = &mut app.overlay else { unreachable!() };
        menu.enter(MenuLevel::Commands);
        menu.list.move_down(); // "History"

        handle_main_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert!(matches!(app.overlay, Some(Overlay::CommandHistory(_))));
    }

    #[test]
    fn handle_main_menu_key_back_at_options_level_returns_to_main() {
        let mut app = app_in_main_menu();
        let Some(Overlay::MainMenu(menu)) = &mut app.overlay else { unreachable!() };
        menu.enter(MenuLevel::Options);

        handle_main_menu_key(&mut app, key(KeyCode::Esc)).unwrap();

        let Some(Overlay::MainMenu(menu)) = &app.overlay else { panic!("expected Overlay::MainMenu") };
        assert_eq!(menu.level, MenuLevel::Main, "should back up a level, not close");
    }

    #[test]
    fn handle_main_menu_key_back_at_main_level_closes_the_menu() {
        let mut app = app_in_main_menu();

        handle_main_menu_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(app.overlay.is_none() && matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn handle_main_menu_key_is_a_noop_outside_main_menu_mode() {
        let mut app = app_in_main_menu();
        app.overlay = None;

        handle_main_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert!(app.overlay.is_none() && matches!(app.mode, Mode::Browsing));
    }
    }
}
