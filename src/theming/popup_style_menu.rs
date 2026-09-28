use color_eyre::eyre::Result;
use crossterm::event::KeyEvent;
use tracing::debug;

use crate::app::{App, Mode};
use crate::choice_menu::{ChoiceMenu, MenuOutcome};
use super::config;
use super::popup_style::PopupStyle;

/// F9 -> Options -> UI: which popup chrome style to use, opened on the
/// active one.
pub type PopupStyleMenu = ChoiceMenu<PopupStyle>;


pub fn open_popup_style_menu(current: PopupStyle) -> PopupStyleMenu {
    ChoiceMenu::new(PopupStyle::all(), Some(current))
}


/// `Enter` applies the highlighted style live (every popup reads
/// `app.popup_style` on the next render) and persists it; `Esc` closes
/// without changing anything.
pub fn handle_popup_style_menu_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Mode::PopupStyleMenu(menu) = &mut app.mode else {
        return Ok(());
    };

    let outcome = menu.handle_key(key);
    debug!(?key, ?outcome, "popup style menu key");
    match outcome {
        MenuOutcome::Open => {}
        MenuOutcome::Closed => app.mode = Mode::Browsing,
        MenuOutcome::Chosen(style) => {
            app.popup_style = style;
            config::set_popup_style(style);
            app.mode = Mode::Browsing;
        }
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;

    use super::*;
    use crate::test_support::key;

    fn app_in_popup_style_menu(current: PopupStyle) -> App {
        let mut app = crate::test_support::test_app(crate::test_support::unique_scratch_dir("popup-style-menu"));
        app.popup_style = current;
        app.mode = Mode::PopupStyleMenu(open_popup_style_menu(current));
        app
    }

    #[test]
    fn opens_on_the_current_style() {
        assert_eq!(open_popup_style_menu(PopupStyle::Classic).selected(), PopupStyle::Classic);
    }

    #[test]
    fn down_moves_the_cursor() {
        let mut app = app_in_popup_style_menu(PopupStyle::Classic);

        handle_popup_style_menu_key(&mut app, key(KeyCode::Down)).unwrap();

        let Mode::PopupStyleMenu(menu) = &app.mode else { panic!("expected Mode::PopupStyleMenu") };
        assert_eq!(menu.selected(), PopupStyle::Rounded);
    }

    #[test]
    fn esc_closes_without_changing_the_style() {
        let mut app = app_in_popup_style_menu(PopupStyle::Rounded);
        let Mode::PopupStyleMenu(menu) = &mut app.mode else { unreachable!() };
        menu.move_up(); // now highlighting Classic, but never applied

        handle_popup_style_menu_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
        assert_eq!(app.popup_style, PopupStyle::Rounded);
    }

    #[test]
    fn is_a_noop_outside_popup_style_menu_mode() {
        let mut app = app_in_popup_style_menu(PopupStyle::Classic);
        app.mode = Mode::Browsing;

        handle_popup_style_menu_key(&mut app, key(KeyCode::Down)).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
    }
}
