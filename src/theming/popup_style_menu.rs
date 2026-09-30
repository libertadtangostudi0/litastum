use color_eyre::eyre::Result;
use crossterm::event::KeyEvent;
use tracing::debug;

use crate::app::{App, Overlay};
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
/// `app.settings.popup_style` on the next render) and persists it; `Esc` closes
/// without changing anything.
pub fn handle_popup_style_menu_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Some(Overlay::PopupStyleMenu(menu)) = &mut app.overlay else {
        return Ok(());
    };

    let outcome = menu.handle_key(key);
    debug!(?key, ?outcome, "popup style menu key");
    match outcome {
        MenuOutcome::Open => {}
        MenuOutcome::Closed => app.overlay = None,
        MenuOutcome::Chosen(style) => {
            app.settings.popup_style = style;
            config::save_settings(&app.settings);
            app.overlay = None;
        }
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;

    use super::*;
    use crate::app::Mode;
    use crate::test_support::key;

    fn app_in_popup_style_menu(current: PopupStyle) -> App {
        let mut app = crate::test_support::test_app(crate::test_support::unique_scratch_dir("popup-style-menu"));
        app.settings.popup_style = current;
        app.overlay = Some(Overlay::PopupStyleMenu(open_popup_style_menu(current)));
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

        let Some(Overlay::PopupStyleMenu(menu)) = &app.overlay else { panic!("expected Overlay::PopupStyleMenu") };
        assert_eq!(menu.selected(), PopupStyle::Rounded);
    }

    #[test]
    fn esc_closes_without_changing_the_style() {
        let mut app = app_in_popup_style_menu(PopupStyle::Rounded);
        let Some(Overlay::PopupStyleMenu(menu)) = &mut app.overlay else { unreachable!() };
        menu.move_up(); // now highlighting Classic, but never applied

        handle_popup_style_menu_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(app.overlay.is_none() && matches!(app.mode, Mode::Browsing));
        assert_eq!(app.settings.popup_style, PopupStyle::Rounded);
    }

    #[test]
    fn is_a_noop_outside_popup_style_menu_mode() {
        let mut app = app_in_popup_style_menu(PopupStyle::Classic);
        app.overlay = None;

        handle_popup_style_menu_key(&mut app, key(KeyCode::Down)).unwrap();

        assert!(app.overlay.is_none() && matches!(app.mode, Mode::Browsing));
    }
}
