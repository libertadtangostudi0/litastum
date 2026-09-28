//! A flat "pick one of these" popup: a fixed list of options, one
//! highlighted, `Up`/`Down` to move, `Enter` to choose, `Esc` to close.
//! The editor's and Compare's F9 menus and their pickers, the UI-style
//! picker and the shell picker are all this; each keeps only its list,
//! its labels and what choosing does.

use crossterm::event::{KeyCode, KeyEvent};


/// What a key did to a `ChoiceMenu`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuOutcome<T> {
    /// `Enter` -- this option was chosen.
    Chosen(T),
    /// `Esc` -- closed without choosing.
    Closed,
    /// Anything else: the highlight may have moved, the menu stays open.
    Open,
}


#[derive(Debug, Clone)]
pub struct ChoiceMenu<T> {
    options: Vec<T>,
    selected: usize,
}


impl<T: Copy + PartialEq> ChoiceMenu<T> {
    /// A menu over `options` (never empty), highlighting `current` if
    /// it's one of them, else the first option.
    pub fn new(options: impl Into<Vec<T>>, current: Option<T>) -> Self {
        let options = options.into();
        assert!(!options.is_empty(), "a choice menu needs at least one option");
        let selected = current.and_then(|current| options.iter().position(|&option| option == current)).unwrap_or(0);
        Self { options, selected }
    }


    pub fn options(&self) -> &[T] {
        &self.options
    }


    pub fn selected_index(&self) -> usize {
        self.selected
    }


    pub fn selected(&self) -> T {
        self.options[self.selected]
    }


    pub fn move_up(&mut self) {
        crate::list_cursor::move_up(&mut self.selected);
    }


    pub fn move_down(&mut self) {
        crate::list_cursor::move_down(&mut self.selected, self.options.len());
    }


    pub fn handle_key(&mut self, key: KeyEvent) -> MenuOutcome<T> {
        match key.code {
            KeyCode::Up => self.move_up(),
            KeyCode::Down => self.move_down(),
            KeyCode::Enter => return MenuOutcome::Chosen(self.selected()),
            KeyCode::Esc => return MenuOutcome::Closed,
            _ => {}
        }
        MenuOutcome::Open
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::key;

    fn menu() -> ChoiceMenu<char> {
        ChoiceMenu::new(['a', 'b', 'c'], None)
    }

    #[test]
    fn opens_on_the_current_option() {
        assert_eq!(ChoiceMenu::new(['a', 'b', 'c'], Some('b')).selected(), 'b');
    }

    #[test]
    fn opens_on_the_first_option_without_a_current_one() {
        assert_eq!(menu().selected(), 'a');
        assert_eq!(ChoiceMenu::new(['a', 'b'], Some('z')).selected(), 'a', "an unknown current falls back to the first");
    }

    #[test]
    fn up_and_down_are_clamped_at_the_ends() {
        let mut menu = menu();
        menu.handle_key(key(KeyCode::Up));
        assert_eq!(menu.selected(), 'a');
        for _ in 0..5 {
            menu.handle_key(key(KeyCode::Down));
        }
        assert_eq!(menu.selected(), 'c');
    }

    #[test]
    fn enter_chooses_the_highlighted_option() {
        let mut menu = menu();
        assert_eq!(menu.handle_key(key(KeyCode::Down)), MenuOutcome::Open);
        assert_eq!(menu.handle_key(key(KeyCode::Enter)), MenuOutcome::Chosen('b'));
    }

    #[test]
    fn esc_closes_and_other_keys_keep_it_open() {
        let mut menu = menu();
        assert_eq!(menu.handle_key(key(KeyCode::Char('z'))), MenuOutcome::Open);
        assert_eq!(menu.selected(), 'a');
        assert_eq!(menu.handle_key(key(KeyCode::Esc)), MenuOutcome::Closed);
    }
}
