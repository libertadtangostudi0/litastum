use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use tracing::debug;

use crate::app::{App, Mode};
use crate::theming::config;

use super::line_ending::LineEndingDisplay;

/// State for Compare's own F9 -> Line endings popup -- mirrors
/// `editor::EditorKeymapMenu`'s own shape exactly (opens with the
/// cursor already on the currently-active choice).
pub struct CompareLineEndingMenu {
    pub selected: usize,
}

impl CompareLineEndingMenu {
    pub fn open(current: LineEndingDisplay) -> Self {
        let selected = LineEndingDisplay::all().iter().position(|&display| display == current).unwrap_or(0);
        Self { selected }
    }

    pub fn move_up(&mut self) {
        crate::list_cursor::move_up(&mut self.selected);
    }

    pub fn move_down(&mut self) {
        crate::list_cursor::move_down(&mut self.selected, LineEndingDisplay::all().len());
    }

    pub fn selected_display(&self) -> LineEndingDisplay {
        LineEndingDisplay::all()[self.selected]
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareLineEndingMenuCommand {
    Up,
    Down,
    Apply,
    Close,
    Ignore,
}

pub fn resolve(key: KeyEvent) -> CompareLineEndingMenuCommand {
    match key.code {
        KeyCode::Up => CompareLineEndingMenuCommand::Up,
        KeyCode::Down => CompareLineEndingMenuCommand::Down,
        KeyCode::Enter => CompareLineEndingMenuCommand::Apply,
        KeyCode::Esc => CompareLineEndingMenuCommand::Close,
        _ => CompareLineEndingMenuCommand::Ignore,
    }
}


/// `Enter` applies the highlighted choice live (`App::compare_line_ending_display`,
/// read directly by `ui/compare.rs::draw_compare` on every frame, no
/// per-pane state to update) and persists it
/// (`config::set_compare_line_ending_display`, best-effort, same as
/// the editor's own keymap-mode picker); `Esc` closes without changing
/// anything. Either way, control returns to `Mode::CompareFiles` with
/// the same `CompareState` this menu was opened over.
pub fn handle_compare_line_ending_menu_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Mode::CompareLineEndingMenu(_, menu) = &mut app.mode else {
        return Ok(());
    };

    let command = resolve(key);
    debug!(?key, ?command, "compare line ending menu key");

    match command {
        CompareLineEndingMenuCommand::Up => menu.move_up(),
        CompareLineEndingMenuCommand::Down => menu.move_down(),
        CompareLineEndingMenuCommand::Close => {
            let Mode::CompareLineEndingMenu(state, _) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
                unreachable!("only called while in Mode::CompareLineEndingMenu");
            };
            app.mode = Mode::CompareFiles(state);
        }
        CompareLineEndingMenuCommand::Apply => {
            let display = menu.selected_display();
            let Mode::CompareLineEndingMenu(state, _) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
                unreachable!("only called while in Mode::CompareLineEndingMenu");
            };
            app.compare_line_ending_display = display;
            config::set_compare_line_ending_display(display);
            app.mode = Mode::CompareFiles(state);
        }
        CompareLineEndingMenuCommand::Ignore => {}
    }

    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::key;

    mod state_tests {
        use super::*;

        #[test]
        fn open_starts_on_the_current_display() {
            let menu = CompareLineEndingMenu::open(LineEndingDisplay::Shown);
            assert_eq!(menu.selected_display(), LineEndingDisplay::Shown);
        }

        #[test]
        fn move_down_clamped_at_the_last_choice() {
            let mut menu = CompareLineEndingMenu::open(LineEndingDisplay::Hidden);
            menu.move_down();
            assert_eq!(menu.selected_display(), LineEndingDisplay::Shown);
            menu.move_down();
            assert_eq!(menu.selected_display(), LineEndingDisplay::Shown);
        }
    }

    mod resolve_tests {
        use super::*;

        #[test]
        fn enter_applies() {
            assert_eq!(resolve(key(KeyCode::Enter)), CompareLineEndingMenuCommand::Apply);
        }

        #[test]
        fn esc_closes() {
            assert_eq!(resolve(key(KeyCode::Esc)), CompareLineEndingMenuCommand::Close);
        }
    }
}
