use crossterm::event::{KeyCode, KeyEvent};


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
    /// F5 — asks to copy the entry under the cursor into the *other*
    /// panel's directory (`Mode::ConfirmTransfer`), Far Manager-style.
    CopySelected,
    /// F6 — same as `CopySelected` but moves instead of copying.
    MoveSelected,
    /// `Shift+F6` — Far Manager's own "Rename or move" binding: opens
    /// the same `Mode::ConfirmTransfer` prompt as `MoveSelected`, but
    /// defaulting the destination to the entry's *own* directory
    /// (rather than the other panel's) so editing just the trailing
    /// name renames it in place. Modifier-specific, so it's resolved
    /// directly in `main.rs::handle_browsing_key` rather than through
    /// this module's `resolve` table (which only keys off `KeyCode`,
    /// not modifiers).
    RenameSelected,
    /// F8 — asks to delete the entry under the cursor
    /// (`Mode::ConfirmDelete`), never deletes directly. Matches Far
    /// Manager's own F8 binding.
    DeleteSelected,
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
        KeyCode::F(5) => Some(Command::CopySelected),
        KeyCode::F(6) => Some(Command::MoveSelected),
        KeyCode::F(8) => Some(Command::DeleteSelected),
        KeyCode::F(9) => Some(Command::OpenMenu),
        // Only F10 quits, matching real Far Manager -- bare letters
        // now type into the always-live command line (main.rs), so a
        // lone 'q' shortcut would swallow the start of typed commands.
        KeyCode::F(10) => Some(Command::Quit),
        _ => None,
    }
}


/// The choice on the "delete this?" prompt (`Mode::ConfirmDelete`) —
/// same Y/N/Esc shape as the editor's `ConfirmDiscardCommand`
/// (`editor_keymap.rs`), kept as its own type rather than shared since
/// this one lives in browsing mode, not the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmDeleteCommand {
    Confirm,
    Cancel,
    /// Anything else — the prompt only understands these two answers.
    Ignore,
}


/// Resolves a raw key press on the delete-confirmation prompt.
pub fn resolve_confirm_delete(key: KeyEvent) -> ConfirmDeleteCommand {
    match key.code {
        KeyCode::Char('y' | 'Y') => ConfirmDeleteCommand::Confirm,
        KeyCode::Char('n' | 'N') | KeyCode::Esc => ConfirmDeleteCommand::Cancel,
        _ => ConfirmDeleteCommand::Ignore,
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

    #[test]
    fn f8_requests_delete() {
        assert_eq!(resolve(KeyCode::F(8)), Some(Command::DeleteSelected));
    }

    #[test]
    fn f5_requests_copy() {
        assert_eq!(resolve(KeyCode::F(5)), Some(Command::CopySelected));
    }

    #[test]
    fn f6_requests_move() {
        assert_eq!(resolve(KeyCode::F(6)), Some(Command::MoveSelected));
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
    }

    #[test]
    fn y_or_uppercase_y_confirms_delete() {
        assert_eq!(resolve_confirm_delete(key(KeyCode::Char('y'))), ConfirmDeleteCommand::Confirm);
        assert_eq!(resolve_confirm_delete(key(KeyCode::Char('Y'))), ConfirmDeleteCommand::Confirm);
    }

    #[test]
    fn n_or_esc_cancels_delete() {
        assert_eq!(resolve_confirm_delete(key(KeyCode::Char('n'))), ConfirmDeleteCommand::Cancel);
        assert_eq!(resolve_confirm_delete(key(KeyCode::Esc)), ConfirmDeleteCommand::Cancel);
    }

    #[test]
    fn other_keys_are_ignored_on_the_delete_prompt() {
        assert_eq!(resolve_confirm_delete(key(KeyCode::Char('x'))), ConfirmDeleteCommand::Ignore);
        assert_eq!(resolve_confirm_delete(key(KeyCode::Enter)), ConfirmDeleteCommand::Ignore);
    }
}
