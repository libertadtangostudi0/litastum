use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use tracing::debug;

use crate::app::{App, Mode};

use super::keymap_menu::EditorKeymapMenu;

/// The built-in editor's own **F9** menu -- distinct from the browsing
/// screen's own F9 (`theming::MainMenu`). Currently just one item,
/// `Keybindings` (leading to `EditorKeymapMenu`'s `Standard`/`Vim`
/// picker), requested directly as a follow-up right after that picker
/// first shipped as F9's own direct target: F9 should open a real
/// submenu instead of jumping straight to the picker. Kept as a real,
/// if currently one-item, list menu rather than collapsing back to a
/// direct jump -- a home for the codepage/whitespace-marker ideas
/// `TODO/editor.md` already floats for this same menu, without needing
/// to restructure this again once one of those actually gets built.
pub struct EditorMenu {
    pub selected: usize,
}

/// The menu's own items, in order -- a plain `&'static [&'static str]`
/// rather than `theming::menu::MenuLevel`'s own enum-of-levels shape,
/// since there's only one level here so far; revisit if a second one
/// is ever added.
const ITEMS: &[&str] = &["Keybindings"];


impl EditorMenu {
    pub fn open() -> Self {
        Self { selected: 0 }
    }


    pub fn move_up(&mut self) {
        crate::list_cursor::move_up(&mut self.selected);
    }


    pub fn move_down(&mut self) {
        crate::list_cursor::move_down(&mut self.selected, ITEMS.len());
    }


    pub fn items(&self) -> &'static [&'static str] {
        ITEMS
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMenuCommand {
    Up,
    Down,
    Select,
    Close,
    Ignore,
}


pub fn resolve(key: KeyEvent) -> EditorMenuCommand {
    match key.code {
        KeyCode::Up => EditorMenuCommand::Up,
        KeyCode::Down => EditorMenuCommand::Down,
        KeyCode::Enter => EditorMenuCommand::Select,
        KeyCode::Esc => EditorMenuCommand::Close,
        _ => EditorMenuCommand::Ignore,
    }
}


/// Key handling for the editor's own F9 menu: `Enter` on `Keybindings`
/// opens `EditorKeymapMenu` (the `Standard`/`Vim` picker) over the same
/// `Editor`; `Esc` closes straight back to `Mode::Editing` -- matching
/// `popup_style_menu.rs`'s own "leaf `Esc` closes all the way out,
/// doesn't step back one level" convention, not `theming::menu.rs`'s
/// own multi-level `back()`, since `EditorKeymapMenu`'s own `Esc`
/// already closes straight to `Mode::Editing` too (unchanged by this
/// menu's own addition) and there's no reason for the two `Esc`
/// behaviors at each level to disagree.
pub fn handle_editor_menu_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Mode::EditorMenu(_, menu) = &mut app.mode else {
        return Ok(());
    };

    let command = resolve(key);
    debug!(?key, ?command, "editor menu key");

    match command {
        EditorMenuCommand::Up => menu.move_up(),
        EditorMenuCommand::Down => menu.move_down(),
        EditorMenuCommand::Close => {
            let Mode::EditorMenu(editor, _) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
                unreachable!("only called while in Mode::EditorMenu");
            };
            app.mode = Mode::Editing(editor);
        }
        EditorMenuCommand::Select => {
            let item = menu.items().get(menu.selected).copied();
            let Mode::EditorMenu(editor, _) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
                unreachable!("only called while in Mode::EditorMenu");
            };
            match item {
                Some("Keybindings") => {
                    let keymap_menu = EditorKeymapMenu::open(editor.keymap_mode());
                    app.mode = Mode::EditorKeymapMenu(editor, keymap_menu);
                }
                _ => app.mode = Mode::Editing(editor),
            }
        }
        EditorMenuCommand::Ignore => {}
    }

    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::{Editor, EditorKeymapMode};
    use crate::test_support::{key, test_app};
    use std::fs;

    fn app_in_editor_menu() -> App {
        let dir = crate::test_support::unique_scratch_dir("editor-menu");
        let path = dir.join("file.txt");
        fs::write(&path, "hello").expect("write test fixture file");
        let editor = Editor::open(path, None, EditorKeymapMode::Standard).expect("open test fixture file");

        let mut app = test_app(dir);
        app.mode = Mode::EditorMenu(editor, EditorMenu::open());
        app
    }

    mod editor_menu_state_tests {
        use super::*;

        #[test]
        fn open_starts_at_the_first_item() {
            let menu = EditorMenu::open();
            assert_eq!(menu.selected, 0);
        }

        #[test]
        fn move_down_clamped_at_the_last_item() {
            let mut menu = EditorMenu::open();
            menu.move_down();
            assert_eq!(menu.selected, 0, "there's currently only one item");
        }

        #[test]
        fn move_up_clamped_at_the_first_item() {
            let mut menu = EditorMenu::open();
            menu.move_up();
            assert_eq!(menu.selected, 0);
        }
    }

    mod resolve_tests {
        use super::*;

        #[test]
        fn enter_selects() {
            assert_eq!(resolve(key(KeyCode::Enter)), EditorMenuCommand::Select);
        }

        #[test]
        fn esc_closes() {
            assert_eq!(resolve(key(KeyCode::Esc)), EditorMenuCommand::Close);
        }

        #[test]
        fn unbound_key_is_ignored() {
            assert_eq!(resolve(key(KeyCode::Char('z'))), EditorMenuCommand::Ignore);
        }
    }

    mod handle_editor_menu_key_tests {
        use super::*;

        #[test]
        fn enter_on_keybindings_opens_the_keymap_picker() {
            let mut app = app_in_editor_menu();

            handle_editor_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

            let Mode::EditorKeymapMenu(editor, menu) = &app.mode else { panic!("expected Mode::EditorKeymapMenu") };
            assert_eq!(editor.keymap_mode(), EditorKeymapMode::Standard);
            assert_eq!(menu.selected_mode(), EditorKeymapMode::Standard);
        }

        #[test]
        fn esc_closes_straight_back_to_editing() {
            let mut app = app_in_editor_menu();

            handle_editor_menu_key(&mut app, key(KeyCode::Esc)).unwrap();

            assert!(matches!(app.mode, Mode::Editing(_)));
        }

        #[test]
        fn handle_editor_menu_key_is_a_noop_outside_editor_menu_mode() {
            let mut app = app_in_editor_menu();
            let Mode::EditorMenu(editor, _) = std::mem::replace(&mut app.mode, Mode::Browsing) else { unreachable!() };
            app.mode = Mode::Editing(editor);

            handle_editor_menu_key(&mut app, key(KeyCode::Enter)).unwrap();

            assert!(matches!(app.mode, Mode::Editing(_)));
        }
    }
}
