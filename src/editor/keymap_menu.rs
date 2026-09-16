use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use tracing::debug;

use crate::app::{App, Mode};
use crate::theming::config;

use super::keymap_mode::EditorKeymapMode;

/// State for the built-in editor's own **F9** popup: which of
/// `EditorKeymapMode::all()` is highlighted. Opens with the cursor
/// already on the currently-active mode, same as `PopupStyleMenu`/
/// `ShellMenu` do for their own current choice, rather than always
/// starting at index 0.
pub struct EditorKeymapMenu {
    pub selected: usize,
}


impl EditorKeymapMenu {
    pub fn open(current: EditorKeymapMode) -> Self {
        let selected = EditorKeymapMode::all().iter().position(|&mode| mode == current).unwrap_or(0);
        Self { selected }
    }


    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }


    pub fn move_down(&mut self) {
        if self.selected + 1 < EditorKeymapMode::all().len() {
            self.selected += 1;
        }
    }


    pub fn selected_mode(&self) -> EditorKeymapMode {
        EditorKeymapMode::all()[self.selected]
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorKeymapMenuCommand {
    Up,
    Down,
    Apply,
    Close,
    Ignore,
}


pub fn resolve(key: KeyEvent) -> EditorKeymapMenuCommand {
    match key.code {
        KeyCode::Up => EditorKeymapMenuCommand::Up,
        KeyCode::Down => EditorKeymapMenuCommand::Down,
        KeyCode::Enter => EditorKeymapMenuCommand::Apply,
        KeyCode::Esc => EditorKeymapMenuCommand::Close,
        _ => EditorKeymapMenuCommand::Ignore,
    }
}


/// Key handling for the editor's own keybinding-mode picker: `Enter`
/// applies the highlighted mode to the editor this menu was opened over
/// (`Editor::set_keymap_mode`, live, no reopen needed) and persists it
/// (`config::set_editor_keymap_mode`, best-effort, same as the popup-
/// style picker's own persistence) as the default for every editor
/// opened from here on (`App::editor_keymap_mode`); `Esc` closes without
/// changing anything. Either way, control returns to `Mode::Editing`
/// with the same `Editor` this menu was opened over -- same
/// `std::mem::replace`-out-then-back shape `Mode::ConfirmDiscard`'s own
/// `Cancel` path already uses to hand an `Editor` back and forth between
/// two `Mode` variants without cloning it.
pub fn handle_editor_keymap_menu_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Mode::EditorKeymapMenu(_, menu) = &mut app.mode else {
        return Ok(());
    };

    let command = resolve(key);
    debug!(?key, ?command, "editor keymap menu key");

    match command {
        EditorKeymapMenuCommand::Up => menu.move_up(),
        EditorKeymapMenuCommand::Down => menu.move_down(),
        EditorKeymapMenuCommand::Close => {
            let Mode::EditorKeymapMenu(editor, _) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
                unreachable!("only called while in Mode::EditorKeymapMenu");
            };
            app.mode = Mode::Editing(editor);
        }
        EditorKeymapMenuCommand::Apply => {
            let mode = menu.selected_mode();
            let Mode::EditorKeymapMenu(mut editor, _) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
                unreachable!("only called while in Mode::EditorKeymapMenu");
            };
            editor.set_keymap_mode(mode);
            app.editor_keymap_mode = mode;
            config::set_editor_keymap_mode(mode);
            app.mode = Mode::Editing(editor);
        }
        EditorKeymapMenuCommand::Ignore => {}
    }

    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::key;

    mod editor_keymap_menu_state_tests {
        use super::*;

        #[test]
        fn open_starts_on_the_current_mode() {
            let menu = EditorKeymapMenu::open(EditorKeymapMode::Vim);
            assert_eq!(menu.selected_mode(), EditorKeymapMode::Vim);
        }

        #[test]
        fn move_down_clamped_at_last_mode() {
            let mut menu = EditorKeymapMenu::open(EditorKeymapMode::Standard);
            menu.move_down();
            assert_eq!(menu.selected_mode(), EditorKeymapMode::Vim);
            menu.move_down();
            assert_eq!(menu.selected_mode(), EditorKeymapMode::Vim);
        }

        #[test]
        fn move_up_clamped_at_first_mode() {
            let mut menu = EditorKeymapMenu::open(EditorKeymapMode::Standard);
            menu.move_up();
            assert_eq!(menu.selected_mode(), EditorKeymapMode::Standard);
        }
    }

    mod resolve_tests {
        use super::*;

        #[test]
        fn enter_applies() {
            assert_eq!(resolve(key(KeyCode::Enter)), EditorKeymapMenuCommand::Apply);
        }

        #[test]
        fn esc_closes() {
            assert_eq!(resolve(key(KeyCode::Esc)), EditorKeymapMenuCommand::Close);
        }

        #[test]
        fn unbound_key_is_ignored() {
            assert_eq!(resolve(key(KeyCode::Char('z'))), EditorKeymapMenuCommand::Ignore);
        }
    }

    mod handle_editor_keymap_menu_key_tests {
        use super::*;
        use crate::editor::Editor;
        use crate::test_support::test_app;
        use std::fs;

        fn app_with_editor_keymap_menu(current: EditorKeymapMode) -> App {
            let dir = crate::test_support::unique_scratch_dir("editor-keymap-menu");
            let path = dir.join("file.txt");
            fs::write(&path, "hello").expect("write test fixture file");
            let editor = Editor::open(path, None, current).expect("open test fixture file");

            let mut app = test_app(dir);
            app.editor_keymap_mode = current;
            app.mode = Mode::EditorKeymapMenu(editor, EditorKeymapMenu::open(current));
            app
        }

        #[test]
        fn down_moves_the_cursor() {
            let mut app = app_with_editor_keymap_menu(EditorKeymapMode::Standard);

            handle_editor_keymap_menu_key(&mut app, key(KeyCode::Down)).unwrap();

            let Mode::EditorKeymapMenu(_, menu) = &app.mode else { panic!("expected Mode::EditorKeymapMenu") };
            assert_eq!(menu.selected_mode(), EditorKeymapMode::Vim);
        }

        #[test]
        fn esc_closes_without_changing_the_mode_and_returns_to_editing() {
            let mut app = app_with_editor_keymap_menu(EditorKeymapMode::Standard);
            let Mode::EditorKeymapMenu(_, menu) = &mut app.mode else { unreachable!() };
            menu.move_down(); // now highlighting Vim, but never applied

            handle_editor_keymap_menu_key(&mut app, key(KeyCode::Esc)).unwrap();

            assert!(matches!(app.mode, Mode::Editing(_)));
            assert_eq!(app.editor_keymap_mode, EditorKeymapMode::Standard);
        }

        #[test]
        fn enter_applies_the_highlighted_mode_to_both_the_app_and_the_live_editor() {
            let mut app = app_with_editor_keymap_menu(EditorKeymapMode::Standard);
            let Mode::EditorKeymapMenu(_, menu) = &mut app.mode else { unreachable!() };
            menu.move_down(); // Vim

            handle_editor_keymap_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

            assert_eq!(app.editor_keymap_mode, EditorKeymapMode::Vim);
            let Mode::Editing(editor) = &app.mode else { panic!("expected Mode::Editing") };
            assert_eq!(editor.keymap_mode(), EditorKeymapMode::Vim);
        }

        #[test]
        fn handle_editor_keymap_menu_key_is_a_noop_outside_editor_keymap_menu_mode() {
            let mut app = app_with_editor_keymap_menu(EditorKeymapMode::Standard);
            let Mode::EditorKeymapMenu(editor, _) = std::mem::replace(&mut app.mode, Mode::Browsing) else { unreachable!() };
            app.mode = Mode::Editing(editor);

            handle_editor_keymap_menu_key(&mut app, key(KeyCode::Down)).unwrap();

            assert!(matches!(app.mode, Mode::Editing(_)));
        }
    }
}
