use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use tracing::debug;

use crate::app::{App, Mode};

use super::line_ending_menu::CompareLineEndingMenu;

/// Compare's own **F9** menu -- mirrors `editor::EditorMenu`'s own
/// shape exactly (a real, if currently one-item, list menu rather than
/// a direct jump to the one picker it leads to), requested directly
/// alongside the line-ending-display feature this exists to reach.
pub struct CompareMenu {
    pub selected: usize,
}

const ITEMS: &[&str] = &["Line endings"];


impl CompareMenu {
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
pub enum CompareMenuCommand {
    Up,
    Down,
    Select,
    Close,
    Ignore,
}

pub fn resolve(key: KeyEvent) -> CompareMenuCommand {
    match key.code {
        KeyCode::Up => CompareMenuCommand::Up,
        KeyCode::Down => CompareMenuCommand::Down,
        KeyCode::Enter => CompareMenuCommand::Select,
        KeyCode::Esc => CompareMenuCommand::Close,
        _ => CompareMenuCommand::Ignore,
    }
}


/// Key handling for Compare's own F9 menu -- same "leaf `Esc` closes
/// all the way out" convention `editor::menu.rs::handle_editor_menu_key`
/// already established, not a multi-level `back()`.
pub fn handle_compare_menu_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Mode::CompareMenu(_, menu) = &mut app.mode else {
        return Ok(());
    };

    let command = resolve(key);
    debug!(?key, ?command, "compare menu key");

    match command {
        CompareMenuCommand::Up => menu.move_up(),
        CompareMenuCommand::Down => menu.move_down(),
        CompareMenuCommand::Close => {
            let Mode::CompareMenu(state, _) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
                unreachable!("only called while in Mode::CompareMenu");
            };
            app.mode = Mode::CompareFiles(state);
        }
        CompareMenuCommand::Select => {
            let item = menu.items().get(menu.selected).copied();
            let Mode::CompareMenu(state, _) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
                unreachable!("only called while in Mode::CompareMenu");
            };
            match item {
                Some("Line endings") => {
                    let line_ending_menu = CompareLineEndingMenu::open(app.compare_line_ending_display);
                    app.mode = Mode::CompareLineEndingMenu(state, line_ending_menu);
                }
                _ => app.mode = Mode::CompareFiles(state),
            }
        }
        CompareMenuCommand::Ignore => {}
    }

    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::key;

    mod compare_menu_state_tests {
        use super::*;

        #[test]
        fn open_starts_at_the_first_item() {
            let menu = CompareMenu::open();
            assert_eq!(menu.selected, 0);
        }

        #[test]
        fn move_down_clamped_at_the_last_item() {
            let mut menu = CompareMenu::open();
            menu.move_down();
            assert_eq!(menu.selected, 0, "there's currently only one item");
        }
    }

    mod resolve_tests {
        use super::*;

        #[test]
        fn enter_selects() {
            assert_eq!(resolve(key(KeyCode::Enter)), CompareMenuCommand::Select);
        }

        #[test]
        fn esc_closes() {
            assert_eq!(resolve(key(KeyCode::Esc)), CompareMenuCommand::Close);
        }
    }
}
