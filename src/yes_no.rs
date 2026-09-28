//! The answer to a Far-style "Y/N" prompt (discard changes, delete,
//! port `FarMenu.ini`): `Y` agrees, `N` or `Esc` declines, anything else
//! is ignored and the prompt stays up.

use crossterm::event::{KeyCode, KeyEvent};


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Yes,
    No,
    /// Not an answer -- the prompt stays open.
    Ignore,
}


pub fn answer(key: KeyEvent) -> Answer {
    match key.code {
        KeyCode::Char('y' | 'Y') => Answer::Yes,
        KeyCode::Char('n' | 'N') | KeyCode::Esc => Answer::No,
        _ => Answer::Ignore,
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::key;

    #[test]
    fn y_in_either_case_agrees() {
        assert_eq!(answer(key(KeyCode::Char('y'))), Answer::Yes);
        assert_eq!(answer(key(KeyCode::Char('Y'))), Answer::Yes);
    }

    #[test]
    fn n_or_esc_declines() {
        assert_eq!(answer(key(KeyCode::Char('n'))), Answer::No);
        assert_eq!(answer(key(KeyCode::Char('N'))), Answer::No);
        assert_eq!(answer(key(KeyCode::Esc)), Answer::No);
    }

    #[test]
    fn anything_else_is_ignored() {
        assert_eq!(answer(key(KeyCode::Char('x'))), Answer::Ignore);
        assert_eq!(answer(key(KeyCode::Enter)), Answer::Ignore, "Enter doesn't pick a side");
    }
}
