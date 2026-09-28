use color_eyre::eyre::Result;
use crossterm::event::KeyEvent;
use tracing::debug;

use crate::app::{App, Mode, Overlay};
use crate::choice_menu::{ChoiceMenu, MenuOutcome};

use super::keymap_menu::open_editor_keymap_menu;

/// An item of the editor's own F9 menu (distinct from the browser's
/// `theming::MainMenu`). One item for now; kept a real menu as the home
/// for more editor settings (`TODO/editor.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMenuItem {
    Keybindings,
}


impl EditorMenuItem {
    pub const ALL: [Self; 1] = [Self::Keybindings];


    pub fn label(self) -> &'static str {
        match self {
            Self::Keybindings => "Keybindings",
        }
    }
}


pub type EditorMenu = ChoiceMenu<EditorMenuItem>;


pub fn open_editor_menu() -> EditorMenu {
    ChoiceMenu::new(EditorMenuItem::ALL, None)
}


/// `Enter` on `Keybindings` opens the keymap picker in its place; `Esc`
/// closes (a leaf `Esc` closes all the way out, like the picker's own).
pub fn handle_editor_menu_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Some(Overlay::EditorMenu(menu)) = &mut app.overlay else {
        return Ok(());
    };

    let outcome = menu.handle_key(key);
    debug!(?key, ?outcome, "editor menu key");
    app.overlay = match outcome {
        MenuOutcome::Open => return Ok(()),
        MenuOutcome::Chosen(EditorMenuItem::Keybindings) => {
            let current = match &app.mode {
                Mode::Editing(editor) => editor.keymap_mode(),
                _ => app.editor_keymap_mode,
            };
            Some(Overlay::EditorKeymapMenu(open_editor_keymap_menu(current)))
        }
        MenuOutcome::Closed => None,
    };
    Ok(())
}


#[cfg(test)]
mod tests {
    use std::fs;

    use crossterm::event::KeyCode;

    use super::*;
    use crate::editor::{Editor, EditorKeymapMode};
    use crate::test_support::{key, test_app};

    fn app_in_editor_menu() -> App {
        let dir = crate::test_support::unique_scratch_dir("editor-menu");
        let path = dir.join("file.txt");
        fs::write(&path, "hello").expect("write test fixture file");
        let editor = Editor::open(path, None, EditorKeymapMode::Standard).expect("open test fixture file");

        let mut app = test_app(dir);
        app.mode = Mode::Editing(editor);
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
