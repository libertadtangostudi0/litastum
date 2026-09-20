use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, Mode};

use super::menu::CompareMenu;

/// Key handling for `Mode::CompareFiles` -- `Up`/`Down`/`PageUp`/
/// `PageDown` scroll both panes together (`CompareState::scroll_row` is
/// shared), `Tab`/`Shift+Tab` jump to the next/previous changed line,
/// `F9` opens Compare's own menu (`super::menu`), `Esc` closes back to
/// `Mode::Browsing`. Nothing here mutates either file -- phase 1 is
/// read-only (`TODO/file-compare.md`).
pub fn handle_compare_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let Mode::CompareFiles(state) = &mut app.mode else {
        return Ok(());
    };

    match key.code {
        KeyCode::Esc => app.mode = Mode::Browsing,
        KeyCode::F(9) => {
            let Mode::CompareFiles(state) = std::mem::replace(&mut app.mode, Mode::Browsing) else {
                unreachable!("just matched Mode::CompareFiles above");
            };
            app.mode = Mode::CompareMenu(state, CompareMenu::open());
        }
        KeyCode::Up => state.scroll_up(1),
        KeyCode::Down => state.scroll_down(1),
        KeyCode::PageUp => state.page_up(),
        KeyCode::PageDown => state.page_down(),
        KeyCode::Tab => state.jump_to_next_hunk(),
        KeyCode::BackTab => state.jump_to_previous_hunk(),
        _ => {}
    }

    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::compare::CompareState;
    use crate::test_support::{key, test_app, unique_scratch_dir};

    fn app_with_compare(left_content: &str, right_content: &str) -> App {
        let dir = unique_scratch_dir("compare-input");
        let left_path = dir.join("left.txt");
        let right_path = dir.join("right.txt");
        std::fs::write(&left_path, left_content).unwrap();
        std::fs::write(&right_path, right_content).unwrap();
        let state = CompareState::open(left_path, right_path).unwrap();

        let mut app = test_app(dir);
        app.mode = Mode::CompareFiles(state);
        app
    }

    #[test]
    fn esc_closes_to_browsing() {
        let mut app = app_with_compare("a\n", "b\n");

        handle_compare_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert!(matches!(app.mode, Mode::Browsing));
    }

    #[test]
    fn down_scrolls_both_panes_together() {
        let mut app = app_with_compare("a\nb\nc\n", "a\nb\nc\n");

        handle_compare_key(&mut app, key(KeyCode::Down)).unwrap();

        let Mode::CompareFiles(state) = &app.mode else { panic!("expected Mode::CompareFiles") };
        assert_eq!(state.scroll_row, 1);
    }

    #[test]
    fn f9_opens_the_compare_menu() {
        let mut app = app_with_compare("a\n", "b\n");

        handle_compare_key(&mut app, key(KeyCode::F(9))).unwrap();

        assert!(matches!(app.mode, Mode::CompareMenu(_, _)));
    }

    #[test]
    fn tab_jumps_to_the_next_hunk() {
        let mut app = app_with_compare("a\nb\nc\n", "a\nx\nc\n");

        handle_compare_key(&mut app, key(KeyCode::Tab)).unwrap();

        let Mode::CompareFiles(state) = &app.mode else { panic!("expected Mode::CompareFiles") };
        assert_eq!(state.scroll_row, 1);
    }
}
