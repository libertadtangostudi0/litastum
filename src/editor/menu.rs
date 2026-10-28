use color_eyre::eyre::Result;
use crossterm::event::KeyEvent;
use tracing::debug;

use crate::app::{App, Mode, Overlay};
use crate::choice_menu::{ChoiceMenu, MenuOutcome};
use crate::command_line::Effect;

use super::keymap_menu::open_editor_keymap_menu;
use super::line_ending_menu::open_line_ending_menu;

/// An item of the editor's own F9 menu (distinct from the browser's
/// `theming::MainMenu`), the home for more editor settings
/// (`TODO/editor.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMenuItem {
    Keybindings,
    LineEndings,
}


impl EditorMenuItem {
    pub const ALL: [Self; 2] = [Self::Keybindings, Self::LineEndings];


    pub fn label(self) -> &'static str {
        match self {
            Self::Keybindings => "Keybindings",
            Self::LineEndings => "Line endings",
        }
    }
}


pub type EditorMenu = ChoiceMenu<EditorMenuItem>;


pub fn open_editor_menu() -> EditorMenu {
    ChoiceMenu::new(EditorMenuItem::ALL, None)
}


/// `Enter` on an item opens its picker in the menu's place; `Esc` closes
/// (a leaf `Esc` closes all the way out, like the pickers' own).
pub fn handle_editor_menu_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    let Some(Overlay::EditorMenu(menu)) = &mut app.overlay else {
        return Ok(Effect::None);
    };

    let outcome = menu.handle_key(key);
    debug!(key = ?crate::logging::LoggedKey(key), ?outcome, "editor menu key");
    app.overlay = match outcome {
        MenuOutcome::Open => return Ok(Effect::None),
        MenuOutcome::Chosen(EditorMenuItem::Keybindings) => {
            let current = match &app.mode {
                Mode::Editing(editor) => editor.keymap_mode(),
                _ => app.settings.editor_keymap_mode,
            };
            Some(Overlay::EditorKeymapMenu(open_editor_keymap_menu(current)))
        }
        MenuOutcome::Chosen(EditorMenuItem::LineEndings) => match &app.mode {
            Mode::Editing(editor) => Some(Overlay::EditorLineEndingMenu(open_line_ending_menu(editor.line_ending()))),
            _ => None,
        },
        MenuOutcome::Closed => None,
    };
    Ok(Effect::None)
}


#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;

    use super::*;
    use crate::editor::EditorKeymapMode;
    use crate::test_support::{editing_app, key};

    fn app_in_editor_menu() -> App {
        let mut app = editing_app("hello", EditorKeymapMode::Standard);
        app.overlay = Some(Overlay::EditorMenu(open_editor_menu()));
        app
    }

    #[test]
    fn enter_on_keybindings_opens_the_keymap_picker() {
        let mut app = app_in_editor_menu();

        handle_editor_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

        let Some(Overlay::EditorKeymapMenu(menu)) = &app.overlay else { panic!("expected the keymap picker") };
        assert_eq!(menu.selected(), EditorKeymapMode::Standard);
        assert!(matches!(app.mode, Mode::Editing(_)), "the editor stays underneath");
    }

    #[test]
    fn enter_on_line_endings_opens_its_picker_on_the_files_own() {
        let mut app = editing_app("a\r\nb\r\n", EditorKeymapMode::Standard);
        app.overlay = Some(Overlay::EditorMenu(open_editor_menu()));

        handle_editor_menu_key(&mut app, key(KeyCode::Down)).unwrap();
        handle_editor_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

        let Some(Overlay::EditorLineEndingMenu(menu)) = &app.overlay else { panic!("expected the line ending picker") };
        assert_eq!(menu.selected(), crate::editor::LineEnding::CrLf);
    }

    #[test]
    fn esc_closes_straight_back_to_editing() {
        let mut app = app_in_editor_menu();

        handle_editor_menu_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(app.overlay.is_none());
        assert!(matches!(app.mode, Mode::Editing(_)));
    }

    #[test]
    fn is_a_noop_without_the_editor_menu_open() {
        let mut app = app_in_editor_menu();
        app.overlay = None;

        handle_editor_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert!(app.overlay.is_none());
    }
}
