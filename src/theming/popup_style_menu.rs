use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use tracing::debug;

use crate::app::{App, Mode};
use super::config;
use super::popup_style::PopupStyle;

/// State for the F9 -> Options -> UI popup: which of `PopupStyle::all()`
/// is highlighted. Opens with the cursor already on the currently-active
/// style, same as `ShellMenu` does for the active shell profile, rather
/// than always starting at index 0.
pub struct PopupStyleMenu {
    pub selected: usize,
}


impl PopupStyleMenu {
    pub fn open(current: PopupStyle) -> Self {
        let selected = PopupStyle::all().iter().position(|&style| style == current).unwrap_or(0);
        Self { selected }
    }


    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }


    pub fn move_down(&mut self) {
        if self.selected + 1 < PopupStyle::all().len() {
            self.selected += 1;
        }
    }


    pub fn selected_style(&self) -> PopupStyle {
        PopupStyle::all()[self.selected]
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupStyleMenuCommand {
    Up,
    Down,
    Apply,
    Close,
    Ignore,
}


pub fn resolve(key: KeyEvent) -> PopupStyleMenuCommand {
    match key.code {
        KeyCode::Up => PopupStyleMenuCommand::Up,
        KeyCode::Down => PopupStyleMenuCommand::Down,
        KeyCode::Enter => PopupStyleMenuCommand::Apply,
        KeyCode::Esc => PopupStyleMenuCommand::Close,
        _ => PopupStyleMenuCommand::Ignore,
    }
}


/// Key handling for the UI-style picker: `Enter` applies the highlighted
/// style immediately (`app.popup_style`, live, no restart -- every popup
/// reads it fresh on the very next render) and persists it
/// (`config::set_popup_style`, best-effort, same as the theme picker's
/// own persistence), `Esc` closes without changing anything.
pub fn handle_popup_style_menu_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Mode::PopupStyleMenu(menu) = &mut app.mode else {
        return Ok(());
    };

    let command = resolve(key);
    debug!(?key, ?command, "popup style menu key");

    match command {
        PopupStyleMenuCommand::Up => menu.move_up(),
        PopupStyleMenuCommand::Down => menu.move_down(),
        PopupStyleMenuCommand::Close => app.mode = Mode::Browsing,
        PopupStyleMenuCommand::Apply => {
            let style = menu.selected_style();
            app.popup_style = style;
            config::set_popup_style(style);
            app.mode = Mode::Browsing;
        }
        PopupStyleMenuCommand::Ignore => {}
    }

    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::key;

    mod popup_style_menu_state_tests {
        use super::*;

        #[test]
        fn open_starts_on_the_current_style() {
            let menu = PopupStyleMenu::open(PopupStyle::Classic);
            assert_eq!(menu.selected_style(), PopupStyle::Classic);
        }

        #[test]
        fn move_down_clamped_at_last_style() {
            let mut menu = PopupStyleMenu::open(PopupStyle::Classic);
            menu.move_down();
            assert_eq!(menu.selected_style(), PopupStyle::Rounded);
            menu.move_down();
            assert_eq!(menu.selected_style(), PopupStyle::Rounded);
        }

        #[test]
        fn move_up_clamped_at_first_style() {
            let mut menu = PopupStyleMenu::open(PopupStyle::Classic);
            menu.move_up();
            assert_eq!(menu.selected_style(), PopupStyle::Classic);
        }
    }

    mod resolve_tests {
        use super::*;

        #[test]
        fn enter_applies() {
            assert_eq!(resolve(key(KeyCode::Enter)), PopupStyleMenuCommand::Apply);
        }

        #[test]
        fn esc_closes() {
            assert_eq!(resolve(key(KeyCode::Esc)), PopupStyleMenuCommand::Close);
        }

        #[test]
        fn unbound_key_is_ignored() {
            assert_eq!(resolve(key(KeyCode::Char('z'))), PopupStyleMenuCommand::Ignore);
        }
    }

    mod handle_popup_style_menu_key_tests {
        use super::*;

        fn app_in_popup_style_menu(current: PopupStyle) -> App {
            let mut app = crate::test_support::test_app(crate::test_support::unique_scratch_dir("popup-style-menu"));
            app.popup_style = current;
            app.mode = Mode::PopupStyleMenu(PopupStyleMenu::open(current));
            app
        }

        #[test]
        fn handle_popup_style_menu_key_down_moves_the_cursor() {
            let mut app = app_in_popup_style_menu(PopupStyle::Classic);

            handle_popup_style_menu_key(&mut app, key(KeyCode::Down)).unwrap();

            let Mode::PopupStyleMenu(menu) = &app.mode else { panic!("expected Mode::PopupStyleMenu") };
            assert_eq!(menu.selected_style(), PopupStyle::Rounded);
        }

        #[test]
        fn handle_popup_style_menu_key_esc_closes_without_changing_the_style() {
            let mut app = app_in_popup_style_menu(PopupStyle::Rounded);
            let Mode::PopupStyleMenu(menu) = &mut app.mode else { unreachable!() };
            menu.move_up(); // now highlighting Classic, but never applied

            handle_popup_style_menu_key(&mut app, key(KeyCode::Esc)).unwrap();

            assert!(matches!(app.mode, Mode::Browsing));
            assert_eq!(app.popup_style, PopupStyle::Rounded);
        }

        #[test]
        fn handle_popup_style_menu_key_is_a_noop_outside_popup_style_menu_mode() {
            let mut app = app_in_popup_style_menu(PopupStyle::Classic);
            app.mode = Mode::Browsing;

            handle_popup_style_menu_key(&mut app, key(KeyCode::Down)).unwrap();

            assert!(matches!(app.mode, Mode::Browsing));
        }
    }
}
