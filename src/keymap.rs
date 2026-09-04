use crossterm::event::KeyCode;


/// A user-triggered action, resolved from a raw key press. Keeps the
/// event loop from growing a `match` arm per new binding, and gives
/// scripting (stage 4 of the roadmap) a typed value to emit instead of
/// a raw key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    EnterSelected,
    ToggleActive,
    EditSelected,
    /// F9 — opens the color-scheme picker (`theme_menu.rs`); a minimal
    /// analog of Far Manager's F9 menu, scoped to just themes for now.
    OpenThemeMenu,
    Quit,
}


/// Maps a raw key press to a `Command`, or `None` if the key isn't bound.
pub fn resolve(key: KeyCode) -> Option<Command> {
    match key {
        KeyCode::Up => Some(Command::MoveUp),
        KeyCode::Down => Some(Command::MoveDown),
        KeyCode::Left => Some(Command::MoveLeft),
        KeyCode::Right => Some(Command::MoveRight),
        KeyCode::Enter => Some(Command::EnterSelected),
        KeyCode::Tab => Some(Command::ToggleActive),
        KeyCode::F(4) => Some(Command::EditSelected),
        KeyCode::F(9) => Some(Command::OpenThemeMenu),
        KeyCode::F(10) | KeyCode::Char('q') => Some(Command::Quit),
        _ => None,
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unbound_key_resolves_to_none() {
        assert_eq!(resolve(KeyCode::Char('z')), None);
    }

    #[test]
    fn f9_opens_the_theme_menu() {
        assert_eq!(resolve(KeyCode::F(9)), Some(Command::OpenThemeMenu));
    }

    #[test]
    fn f10_and_q_both_resolve_to_quit() {
        assert_eq!(resolve(KeyCode::F(10)), Some(Command::Quit));
        assert_eq!(resolve(KeyCode::Char('q')), Some(Command::Quit));
    }
}
