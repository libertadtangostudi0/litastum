use color_eyre::eyre::Result;
use crossterm::event::KeyEvent;
use tracing::debug;

use crate::app::{App, Mode, Overlay};
use crate::choice_menu::{ChoiceMenu, MenuOutcome};
use crate::theming::config;

use super::keymap_mode::EditorKeymapMode;

/// The editor's F9 -> Keybindings picker, opened on the active mode.
pub type EditorKeymapMenu = ChoiceMenu<EditorKeymapMode>;


pub fn open_editor_keymap_menu(current: EditorKeymapMode) -> EditorKeymapMenu {
    ChoiceMenu::new(EditorKeymapMode::all(), Some(current))
}


/// `Enter` applies the highlighted mode to the editor underneath (live)
/// and persists it as the default for new editors; `Esc` closes without
/// changing anything.
pub fn handle_editor_keymap_menu_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Some(Overlay::EditorKeymapMenu(menu)) = &mut app.overlay else {
        return Ok(());
    };

    let outcome = menu.handle_key(key);
    debug!(?key, ?outcome, "editor keymap menu key");
    if outcome == MenuOutcome::Open {
        return Ok(());
    }

    app.overlay = None;
    if let MenuOutcome::Chosen(mode) = outcome {
        if let Mode::Editing(editor) = &mut app.mode {
            editor.set_keymap_mode(mode);
        }
        app.editor_keymap_mode = mode;
        config::set_editor_keymap_mode(mode);
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use std::fs;

    use crossterm::event::KeyCode;

    use super::*;
    use crate::editor::Editor;
    use crate::test_support::{key, test_app};

    fn app_with_editor_keymap_menu(current: EditorKeymapMode) -> App {
        let dir = crate::test_support::unique_scratch_dir("editor-keymap-menu");
        let path = dir.join("file.txt");
        fs::write(&path, "hello").expect("write test fixture file");
        let editor = Editor::open(path, None, current).expect("open test fixture file");

        let mut app = test_app(dir);
        app.editor_keymap_mode = current;
        app.mode = Mode::Editing(editor);
        app.overlay = Some(Overlay::EditorKeymapMenu(open_editor_keymap_menu(current)));
        app
    }

    #[test]
    fn opens_on_the_current_mode() {
        assert_eq!(open_editor_keymap_menu(EditorKeymapMode::Vim).selected(), EditorKeymapMode::Vim);
    }

    #[test]
    fn down_moves_the_cursor() {
        let mut app = app_with_editor_keymap_menu(EditorKeymapMode::Standard);

        handle_editor_keymap_menu_key(&mut app, key(KeyCode::Down)).unwrap();

        let Some(Overlay::EditorKeymapMenu(menu)) = &app.overlay else { panic!("expected the keymap picker") };
        assert_eq!(menu.selected(), EditorKeymapMode::Vim);
    }

    #[test]
    fn esc_closes_without_changing_the_mode_and_returns_to_editing() {
        let mut app = app_with_editor_keymap_menu(EditorKeymapMode::Standard);
        let Some(Overlay::EditorKeymapMenu(menu)) = &mut app.overlay else { unreachable!() };
        menu.move_down(); // now highlighting Vim, but never applied

        handle_editor_keymap_menu_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(app.overlay.is_none());
        assert_eq!(app.editor_keymap_mode, EditorKeymapMode::Standard);
    }

    #[test]
    fn enter_applies_the_highlighted_mode_to_both_the_app_and_the_live_editor() {
        let mut app = app_with_editor_keymap_menu(EditorKeymapMode::Standard);
        let Some(Overlay::EditorKeymapMenu(menu)) = &mut app.overlay else { unreachable!() };
        menu.move_down(); // Vim

        handle_editor_keymap_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert_eq!(app.editor_keymap_mode, EditorKeymapMode::Vim);
        let Mode::Editing(editor) = &app.mode else { panic!("expected Mode::Editing") };
        assert_eq!(editor.keymap_mode(), EditorKeymapMode::Vim);
    }

    #[test]
    fn is_a_noop_without_the_picker_open() {
        let mut app = app_with_editor_keymap_menu(EditorKeymapMode::Standard);
        app.overlay = None;

        handle_editor_keymap_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert_eq!(app.editor_keymap_mode, EditorKeymapMode::Standard);
    }
}
