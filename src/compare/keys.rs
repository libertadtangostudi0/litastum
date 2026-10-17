//! The keys of the two diff screens, Compare and the conflict resolver --
//! one table, so they can't drift apart (they had: `Ctrl`/`Alt`+arrows
//! were changed in each by hand). Each screen acts on the commands it
//! has; the rest of a key goes to the focused pane (`editor::text_key`).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::editor::edtui_supports_key;


/// A diff screen's command, before the focused pane's own keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffCommand {
    Close,
    Save,
    SaveAs,
    /// `Ctrl+F2`/`Ctrl+L` -- the focused pane's path as a field.
    EditPath,
    /// `Tab`/`Shift+Tab` -- the next/previous pane.
    NextPane,
    PreviousPane,
    /// `F8`/`F7`, `Alt+Down`/`Alt+Up` -- the next/previous change
    /// (TortoiseMerge/`merge.exe`'s keys, requested); `Ctrl+Up`/`Down`
    /// move by blocks of code, as in F4.
    NextChange,
    PreviousChange,
    /// `F9` -- the screen's menu, where it has one.
    OpenMenu,
    /// For the focused pane.
    Forward,
    /// A key `edtui` can't handle -- swallowed.
    Ignore,
}


pub(crate) fn resolve(key: KeyEvent) -> DiffCommand {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    match key.code {
        KeyCode::Esc => DiffCommand::Close,
        KeyCode::Char('s' | 'S') if ctrl => DiffCommand::Save,
        // Ctrl+F2 edits the path, as on a panel's title; above plain F2 (save).
        KeyCode::F(2) if ctrl => DiffCommand::EditPath,
        KeyCode::F(2) if shift => DiffCommand::SaveAs,
        KeyCode::F(2) => DiffCommand::Save,
        KeyCode::Char('l' | 'L') if ctrl => DiffCommand::EditPath,
        KeyCode::Tab => DiffCommand::NextPane,
        KeyCode::BackTab => DiffCommand::PreviousPane,
        KeyCode::Down if alt => DiffCommand::NextChange,
        KeyCode::Up if alt => DiffCommand::PreviousChange,
        KeyCode::F(8) => DiffCommand::NextChange,
        KeyCode::F(7) => DiffCommand::PreviousChange,
        KeyCode::F(9) => DiffCommand::OpenMenu,
        _ if edtui_supports_key(key.code) => DiffCommand::Forward,
        _ => DiffCommand::Ignore,
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn with(code: KeyCode, modifiers: KeyModifiers) -> DiffCommand {
        resolve(KeyEvent::new(code, modifiers))
    }

    #[test]
    fn the_changes_are_on_f7_f8_and_alt_arrows_and_ctrl_arrows_go_to_the_pane() {
        assert_eq!(with(KeyCode::F(8), KeyModifiers::NONE), DiffCommand::NextChange);
        assert_eq!(with(KeyCode::Down, KeyModifiers::ALT), DiffCommand::NextChange);
        assert_eq!(with(KeyCode::Up, KeyModifiers::ALT), DiffCommand::PreviousChange);
        assert_eq!(with(KeyCode::Down, KeyModifiers::CONTROL), DiffCommand::Forward, "by blocks, the pane's");
    }

    #[test]
    fn f2_saves_shift_f2_saves_as_ctrl_f2_edits_the_path() {
        assert_eq!(with(KeyCode::F(2), KeyModifiers::NONE), DiffCommand::Save);
        assert_eq!(with(KeyCode::F(2), KeyModifiers::SHIFT), DiffCommand::SaveAs);
        assert_eq!(with(KeyCode::F(2), KeyModifiers::CONTROL), DiffCommand::EditPath);
        assert_eq!(with(KeyCode::Char('l'), KeyModifiers::CONTROL), DiffCommand::EditPath);
    }

    #[test]
    fn a_key_edtui_cant_take_is_swallowed() {
        assert_eq!(with(KeyCode::F(10), KeyModifiers::NONE), DiffCommand::Ignore);
    }
}
