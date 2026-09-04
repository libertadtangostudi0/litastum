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
    /// F9 — opens the top menu (`menu.rs`), currently `Settings` →
    /// `Color schemes` (`theme_menu.rs`); a minimal analog of Far
    /// Manager's F9 menu, scoped to just that path for now.
    OpenMenu,
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
        KeyCode::F(9) => Some(Command::OpenMenu),
        // Only F10 quits, matching real Far Manager -- bare letters
        // now type into the always-live command line (main.rs), so a
        // lone 'q' shortcut would swallow the start of typed commands.
        KeyCode::F(10) => Some(Command::Quit),
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
    fn f9_opens_the_menu() {
        assert_eq!(resolve(KeyCode::F(9)), Some(Command::OpenMenu));
    }

    #[test]
    fn f10_quits() {
        assert_eq!(resolve(KeyCode::F(10)), Some(Command::Quit));
    }

    #[test]
    fn bare_q_no_longer_quits() {
        // Regression guard: 'q' used to be a quick-quit shortcut, but
        // now types into the always-live command line (main.rs) like
        // any other letter -- only F10 quits, matching real Far
        // Manager. See ARCHITECTURE.md / the plan for this feature.
        assert_eq!(resolve(KeyCode::Char('q')), None);
    }
}
