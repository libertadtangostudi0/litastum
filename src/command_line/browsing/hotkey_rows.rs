//! The browser's rows of F9 -> Options -> Hotkeys, read off the binding
//! table itself (`bindings::BINDINGS`), so the list can't drift from what
//! the keys really do.

use crossterm::event::{KeyCode, KeyModifiers};

use crate::explorer::Command;
use crate::hotkeys::HotkeyRow;

use super::bindings::{BrowserAction, When, BINDINGS};


/// Every browser binding, in the table's order; a key listed twice only
/// for its case (`Ctrl+l`/`Ctrl+L`) shows once.
pub(crate) fn browser_hotkeys() -> Vec<HotkeyRow> {
    let mut rows: Vec<HotkeyRow> = Vec::new();
    for binding in BINDINGS {
        let row = HotkeyRow { keys: key_label(binding.code, binding.required), action: action_label(binding.action), when: when_label(binding.when) };
        if !rows.contains(&row) {
            rows.push(row);
        }
    }
    rows
}


/// `Ctrl+Shift+Left`, `Alt+F7`, `Enter` -- modifiers in a fixed order.
pub(crate) fn key_label(code: KeyCode, modifiers: KeyModifiers) -> String {
    let mut label = String::new();
    for (modifier, name) in [(KeyModifiers::CONTROL, "Ctrl+"), (KeyModifiers::ALT, "Alt+"), (KeyModifiers::SHIFT, "Shift+")] {
        if modifiers.contains(modifier) {
            label.push_str(name);
        }
    }
    let key = match code {
        KeyCode::Char(c) => c.to_uppercase().to_string(),
        KeyCode::F(n) => format!("F{n}"),
        KeyCode::Delete => "Del".to_string(),
        KeyCode::Insert => "Ins".to_string(),
        KeyCode::PageUp => "PgUp".to_string(),
        KeyCode::PageDown => "PgDn".to_string(),
        other => format!("{other:?}"),
    };
    label.push_str(&key);
    label
}


fn when_label(when: When) -> &'static str {
    match when {
        When::Always => "",
        When::EmptyLine => "empty command line",
        When::TypedLine => "something typed",
        When::SuggestionsShowing => "suggestions shown",
    }
}


fn action_label(action: BrowserAction) -> &'static str {
    use BrowserAction as A;
    match action {
        A::Command(command) | A::Navigate(command) => command_label(command),
        A::ToggleHiddenPanels => "Hide / show the panels",
        A::OpenShellMenu => "Pick the shell",
        A::OpenFindFile => "Find file",
        A::OpenDriveMenu(0) => "Change drive, left panel",
        A::OpenDriveMenu(_) => "Change drive, right panel",
        A::OpenHistory => "Command history",
        A::CompareFiles => "Compare files",
        A::EditPanelPath => "Edit the panel's path",
        A::SelectWordLeft => "Select a word left",
        A::SelectWordRight => "Select a word right",
        A::SelectLeft => "Select a character left",
        A::SelectRight => "Select a character right",
        A::WordLeft => "A word left",
        A::WordRight => "A word right",
        A::CopySelection => "Copy the selection",
        A::CutSelection => "Cut the selection",
        A::SelectLine => "Select the whole line",
        A::Submit => "Run the command",
        A::SuggestionUp => "Previous suggestion",
        A::SuggestionDown => "Next suggestion",
        A::AcceptSuggestion => "Take the suggestion",
        A::DeleteSuggestion => "Forget the history entry",
        A::EditSuggestion => "Edit the suggested file",
        A::Complete => "Complete the path",
        A::ClearLine => "Clear the command line",
        A::Backspace => "Delete back",
        A::DeleteForward => "Delete forward",
    }
}


fn command_label(command: Command) -> &'static str {
    match command {
        Command::MoveUp => "Up",
        Command::MoveDown => "Down",
        Command::MoveLeft => "Previous column",
        Command::MoveRight => "Next column",
        Command::EnterSelected => "Open the directory or file",
        Command::OpenInFileManager => "Open in the file manager",
        Command::ToggleActive => "Other panel",
        Command::EditSelected => "Edit",
        Command::PreviewSelected => "View",
        Command::OpenUserMenu => "User menu",
        Command::OpenMenu => "Menu",
        Command::CopySelected => "Copy",
        Command::MoveSelected => "Move",
        Command::RenameSelected => "Rename or move",
        Command::DeleteSelected => "Delete to the Recycle Bin",
        Command::WipeSelected => "Delete for good",
        Command::SelectAll => "Mark all",
        Command::MarkMoveUp => "Mark and go up",
        Command::MarkMoveDown => "Mark and go down",
        Command::MarkMoveLeft => "Mark to the previous column",
        Command::MarkMoveRight => "Mark to the next column",
        Command::SwapPanels => "Swap the panels",
        Command::Quit => "Quit",
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn row<'a>(rows: &'a [HotkeyRow], keys: &str, when: &str) -> Option<&'a HotkeyRow> {
        rows.iter().find(|row| row.keys == keys && row.when == when)
    }

    #[test]
    fn every_binding_is_listed_with_what_it_does() {
        let rows = browser_hotkeys();
        assert_eq!(row(&rows, "Alt+F7", "").map(|row| row.action), Some("Find file"));
        assert_eq!(row(&rows, "F8", "").map(|row| row.action), Some("Delete to the Recycle Bin"));
        assert_eq!(row(&rows, "F8", "suggestions shown").map(|row| row.action), Some("Forget the history entry"));
        assert_eq!(row(&rows, "Ctrl+Shift+Left", "").map(|row| row.action), Some("Select a word left"));
        assert_eq!(row(&rows, "Shift+Del", "empty command line").map(|row| row.action), Some("Delete for good"));
    }

    #[test]
    fn a_key_bound_in_both_cases_shows_once() {
        let rows = browser_hotkeys();
        assert_eq!(rows.iter().filter(|row| row.keys == "Ctrl+L").count(), 1);
        assert_eq!(rows.iter().filter(|row| row.keys == "Shift+A").count(), 1);
    }
}
