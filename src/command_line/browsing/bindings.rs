//! The browser's key bindings as one ordered table: a key, the modifiers
//! it needs (and must not have), a condition on the command line, and
//! the action. The first matching row wins, so a specific chord sits
//! above the plain key it would otherwise fall through to (`Alt+F5`
//! above `F5`, `Shift+F6` above `F6`). Unmatched keys type into the
//! command line (`handle_browsing_key`).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::explorer::Command;


/// What has to be true of the command line for a binding to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum When {
    Always,
    EmptyLine,
    TypedLine,
    /// The history suggestions overlay is showing.
    SuggestionsShowing,
}


/// The command line's state, as far as `When` cares.
#[derive(Debug, Clone, Copy)]
pub(super) struct LineState {
    pub empty: bool,
    pub suggestions_showing: bool,
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BrowserAction {
    /// A panel command, run as is.
    Command(Command),
    /// A panel command from the plain navigation/F-key row: also ends
    /// any completion cycle or suggestion browsing and drops the command
    /// line's selection, since focus moves to the panels.
    Navigate(Command),
    ToggleHiddenPanels,
    OpenShellMenu,
    OpenFindFile,
    /// `Alt+F1`/`Alt+F2`: always that panel (0/1), not the focused one.
    OpenDriveMenu(usize),
    OpenHistory,
    CompareFiles,
    /// `Ctrl+L`/`Ctrl+F2`: the active panel's path title as a field.
    EditPanelPath,
    SelectWordLeft,
    SelectWordRight,
    SelectLeft,
    SelectRight,
    WordLeft,
    WordRight,
    CopySelection,
    CutSelection,
    Submit,
    SuggestionUp,
    SuggestionDown,
    AcceptSuggestion,
    /// `F8` with the suggestions showing: forget the highlighted one
    /// (above `F8` = Delete files).
    DeleteSuggestion,
    /// `F4` with the suggestions showing: a highlighted panel file is
    /// selected in the panel and opened in the editor; otherwise plain
    /// `F4`.
    EditSuggestion,
    Complete,
    ClearLine,
    Backspace,
    DeleteForward,
}


pub(super) struct Binding {
    code: KeyCode,
    required: KeyModifiers,
    forbidden: KeyModifiers,
    when: When,
    action: BrowserAction,
}


const NONE: KeyModifiers = KeyModifiers::NONE;
const CTRL: KeyModifiers = KeyModifiers::CONTROL;
const SHIFT: KeyModifiers = KeyModifiers::SHIFT;
const ALT: KeyModifiers = KeyModifiers::ALT;
const CTRL_SHIFT: KeyModifiers = KeyModifiers::CONTROL.union(KeyModifiers::SHIFT);


/// Needs `required` held; other modifiers don't matter.
const fn with(code: KeyCode, required: KeyModifiers, when: When, action: BrowserAction) -> Binding {
    Binding { code, required, forbidden: NONE, when, action }
}


/// Needs `required` held and none of `forbidden`.
const fn without(code: KeyCode, required: KeyModifiers, forbidden: KeyModifiers, when: When, action: BrowserAction) -> Binding {
    Binding { code, required, forbidden, when, action }
}


/// Any modifiers -- for rows below every chord on the same key.
const fn any(code: KeyCode, when: When, action: BrowserAction) -> Binding {
    Binding { code, required: NONE, forbidden: NONE, when, action }
}


use BrowserAction as A;
use When::{Always, EmptyLine, SuggestionsShowing, TypedLine};

pub(super) static BINDINGS: &[Binding] = &[
    // Modifier chords.
    with(KeyCode::Char('o'), CTRL, Always, A::ToggleHiddenPanels),
    with(KeyCode::Char('p'), CTRL, Always, A::OpenShellMenu),
    with(KeyCode::Char('u'), CTRL, Always, A::Command(Command::SwapPanels)),
    with(KeyCode::F(6), SHIFT, Always, A::Command(Command::RenameSelected)),
    with(KeyCode::Enter, SHIFT, EmptyLine, A::Command(Command::OpenInFileManager)),
    with(KeyCode::F(7), ALT, Always, A::OpenFindFile),
    with(KeyCode::F(1), ALT, Always, A::OpenDriveMenu(0)),
    with(KeyCode::F(2), ALT, Always, A::OpenDriveMenu(1)),
    with(KeyCode::F(8), ALT, Always, A::OpenHistory),
    with(KeyCode::F(5), ALT, Always, A::CompareFiles),
    with(KeyCode::Char('l'), CTRL, Always, A::EditPanelPath),
    with(KeyCode::Char('L'), CTRL, Always, A::EditPanelPath),
    with(KeyCode::F(2), CTRL, Always, A::EditPanelPath),
    // Selection and word moves inside the command line. Bare arrows stay
    // panel navigation; Shift+Left/Right on an empty line mark instead.
    with(KeyCode::Left, CTRL_SHIFT, Always, A::SelectWordLeft),
    with(KeyCode::Right, CTRL_SHIFT, Always, A::SelectWordRight),
    without(KeyCode::Left, SHIFT, CTRL, TypedLine, A::SelectLeft),
    without(KeyCode::Right, SHIFT, CTRL, TypedLine, A::SelectRight),
    without(KeyCode::Left, CTRL, SHIFT, Always, A::WordLeft),
    without(KeyCode::Right, CTRL, SHIFT, Always, A::WordRight),
    // Clipboard, Windows and Far style. Above `Delete` = delete forward.
    with(KeyCode::Char('c'), CTRL, TypedLine, A::CopySelection),
    with(KeyCode::Insert, CTRL, TypedLine, A::CopySelection),
    with(KeyCode::Char('x'), CTRL, TypedLine, A::CutSelection),
    with(KeyCode::Delete, SHIFT, TypedLine, A::CutSelection),
    // Marking (`panel/marks.rs`). Shift+A only on an empty line, or a
    // command could never start with a capital letter.
    without(KeyCode::Char('a'), SHIFT, CTRL, EmptyLine, A::Command(Command::SelectAll)),
    without(KeyCode::Char('A'), SHIFT, CTRL, EmptyLine, A::Command(Command::SelectAll)),
    without(KeyCode::Up, SHIFT, CTRL, Always, A::Command(Command::MarkMoveUp)),
    without(KeyCode::Down, SHIFT, CTRL, Always, A::Command(Command::MarkMoveDown)),
    without(KeyCode::Left, SHIFT, CTRL, Always, A::Command(Command::MarkMoveLeft)),
    without(KeyCode::Right, SHIFT, CTRL, Always, A::Command(Command::MarkMoveRight)),
    // The typed line.
    any(KeyCode::Enter, TypedLine, A::Submit),
    // `Enter` isn't here on purpose: it always runs exactly what's typed.
    any(KeyCode::Up, SuggestionsShowing, A::SuggestionUp),
    any(KeyCode::Down, SuggestionsShowing, A::SuggestionDown),
    any(KeyCode::Tab, SuggestionsShowing, A::AcceptSuggestion),
    any(KeyCode::F(8), SuggestionsShowing, A::DeleteSuggestion),
    without(KeyCode::F(4), NONE, CTRL.union(ALT).union(SHIFT), SuggestionsShowing, A::EditSuggestion),
    any(KeyCode::Tab, TypedLine, A::Complete),
    // Panel navigation and the F-key row (Far's bare F-keys; F10 is the
    // only way to quit, since letters type into the command line).
    any(KeyCode::Up, Always, A::Navigate(Command::MoveUp)),
    any(KeyCode::Down, Always, A::Navigate(Command::MoveDown)),
    any(KeyCode::Left, Always, A::Navigate(Command::MoveLeft)),
    any(KeyCode::Right, Always, A::Navigate(Command::MoveRight)),
    any(KeyCode::Enter, Always, A::Navigate(Command::EnterSelected)),
    any(KeyCode::Tab, Always, A::Navigate(Command::ToggleActive)),
    any(KeyCode::F(2), Always, A::Navigate(Command::OpenUserMenu)),
    any(KeyCode::F(3), Always, A::Navigate(Command::PreviewSelected)),
    any(KeyCode::F(4), Always, A::Navigate(Command::EditSelected)),
    any(KeyCode::F(5), Always, A::Navigate(Command::CopySelected)),
    any(KeyCode::F(6), Always, A::Navigate(Command::MoveSelected)),
    any(KeyCode::F(8), Always, A::Navigate(Command::DeleteSelected)),
    any(KeyCode::F(9), Always, A::Navigate(Command::OpenMenu)),
    any(KeyCode::F(10), Always, A::Navigate(Command::Quit)),
    // Editing the command line.
    any(KeyCode::Esc, Always, A::ClearLine),
    any(KeyCode::Backspace, Always, A::Backspace),
    any(KeyCode::Delete, Always, A::DeleteForward),
];


impl Binding {
    fn matches(&self, key: KeyEvent, line: LineState) -> bool {
        key.code == self.code
            && key.modifiers.contains(self.required)
            && !key.modifiers.intersects(self.forbidden)
            && match self.when {
                Always => true,
                EmptyLine => line.empty,
                TypedLine => !line.empty,
                SuggestionsShowing => line.suggestions_showing,
            }
    }
}


/// The first binding matching `key` in this state, if any.
pub(super) fn lookup(key: KeyEvent, line: LineState) -> Option<BrowserAction> {
    BINDINGS.iter().find(|binding| binding.matches(key, line)).map(|binding| binding.action)
}


#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY: LineState = LineState { empty: true, suggestions_showing: false };
    const TYPED: LineState = LineState { empty: false, suggestions_showing: false };
    const SUGGESTING: LineState = LineState { empty: false, suggestions_showing: true };

    fn chord(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn clipboard_chords_work_only_on_a_typed_line() {
        assert_eq!(lookup(chord(KeyCode::Char('c'), CTRL), TYPED), Some(A::CopySelection));
        assert_eq!(lookup(chord(KeyCode::Insert, CTRL), TYPED), Some(A::CopySelection));
        assert_eq!(lookup(chord(KeyCode::Char('x'), CTRL), TYPED), Some(A::CutSelection));
        assert_eq!(lookup(chord(KeyCode::Delete, SHIFT), TYPED), Some(A::CutSelection));
        assert_eq!(lookup(chord(KeyCode::Delete, NONE), TYPED), Some(A::DeleteForward));
        assert_eq!(lookup(chord(KeyCode::Char('c'), CTRL), EMPTY), None);
    }

    #[test]
    fn a_chord_wins_over_the_plain_key_below_it() {
        assert_eq!(lookup(chord(KeyCode::F(5), ALT), EMPTY), Some(A::CompareFiles));
        assert_eq!(lookup(chord(KeyCode::F(5), NONE), EMPTY), Some(A::Navigate(Command::CopySelected)));
        assert_eq!(lookup(chord(KeyCode::F(6), SHIFT), EMPTY), Some(A::Command(Command::RenameSelected)));
        assert_eq!(lookup(chord(KeyCode::F(6), NONE), EMPTY), Some(A::Navigate(Command::MoveSelected)));
    }

    #[test]
    fn shift_arrows_select_while_typing_and_mark_on_an_empty_line() {
        assert_eq!(lookup(chord(KeyCode::Left, SHIFT), TYPED), Some(A::SelectLeft));
        assert_eq!(lookup(chord(KeyCode::Left, SHIFT), EMPTY), Some(A::Command(Command::MarkMoveLeft)));
        assert_eq!(lookup(chord(KeyCode::Left, CTRL_SHIFT), EMPTY), Some(A::SelectWordLeft));
        assert_eq!(lookup(chord(KeyCode::Up, SHIFT), TYPED), Some(A::Command(Command::MarkMoveUp)), "Shift+Up always marks");
    }

    #[test]
    fn enter_and_tab_depend_on_the_line() {
        assert_eq!(lookup(chord(KeyCode::Enter, NONE), TYPED), Some(A::Submit));
        assert_eq!(lookup(chord(KeyCode::Enter, NONE), EMPTY), Some(A::Navigate(Command::EnterSelected)));
        assert_eq!(lookup(chord(KeyCode::Enter, SHIFT), EMPTY), Some(A::Command(Command::OpenInFileManager)));
        assert_eq!(lookup(chord(KeyCode::Enter, SHIFT), TYPED), Some(A::Submit), "Shift+Enter while typing just runs the line");
        assert_eq!(lookup(chord(KeyCode::Tab, NONE), SUGGESTING), Some(A::AcceptSuggestion));
        assert_eq!(lookup(chord(KeyCode::Tab, NONE), TYPED), Some(A::Complete));
        assert_eq!(lookup(chord(KeyCode::Tab, NONE), EMPTY), Some(A::Navigate(Command::ToggleActive)));
    }

    #[test]
    fn suggestions_take_up_and_down_but_never_enter() {
        assert_eq!(lookup(chord(KeyCode::Up, NONE), SUGGESTING), Some(A::SuggestionUp));
        assert_eq!(lookup(chord(KeyCode::Enter, NONE), SUGGESTING), Some(A::Submit));
        assert_eq!(lookup(chord(KeyCode::Up, NONE), TYPED), Some(A::Navigate(Command::MoveUp)));
    }

    #[test]
    fn f8_forgets_a_suggestion_rather_than_deleting_files() {
        assert_eq!(lookup(chord(KeyCode::F(8), NONE), SUGGESTING), Some(A::DeleteSuggestion));
        assert_eq!(lookup(chord(KeyCode::F(8), NONE), TYPED), Some(A::Navigate(Command::DeleteSelected)));
        assert_eq!(lookup(chord(KeyCode::F(8), ALT), SUGGESTING), Some(A::OpenHistory), "Alt+F8 stays the history popup");
    }

    #[test]
    fn ctrl_f2_edits_the_panel_path_and_f2_stays_the_user_menu() {
        assert_eq!(lookup(chord(KeyCode::F(2), CTRL), EMPTY), Some(A::EditPanelPath));
        assert_eq!(lookup(chord(KeyCode::F(2), CTRL), TYPED), Some(A::EditPanelPath));
        assert_eq!(lookup(chord(KeyCode::F(2), NONE), EMPTY), Some(A::Navigate(Command::OpenUserMenu)));
    }

    #[test]
    fn f4_works_on_a_suggestion_only_while_they_show() {
        assert_eq!(lookup(chord(KeyCode::F(4), NONE), SUGGESTING), Some(A::EditSuggestion));
        assert_eq!(lookup(chord(KeyCode::F(4), NONE), TYPED), Some(A::Navigate(Command::EditSelected)));
    }

    #[test]
    fn shift_a_is_only_a_binding_on_an_empty_line() {
        assert_eq!(lookup(chord(KeyCode::Char('A'), SHIFT), EMPTY), Some(A::Command(Command::SelectAll)));
        assert_eq!(lookup(chord(KeyCode::Char('A'), SHIFT), TYPED), None, "falls through to typing");
    }

    #[test]
    fn the_f_key_row() {
        let f = |n| lookup(chord(KeyCode::F(n), NONE), EMPTY);
        assert_eq!(f(2), Some(A::Navigate(Command::OpenUserMenu)));
        assert_eq!(f(3), Some(A::Navigate(Command::PreviewSelected)));
        assert_eq!(f(4), Some(A::Navigate(Command::EditSelected)));
        assert_eq!(f(8), Some(A::Navigate(Command::DeleteSelected)));
        assert_eq!(f(9), Some(A::Navigate(Command::OpenMenu)));
        assert_eq!(f(10), Some(A::Navigate(Command::Quit)));
    }

    /// Regression guard: `q` used to quit; now only F10 does, since
    /// letters type into the always-live command line.
    #[test]
    fn plain_letters_are_not_bound() {
        assert_eq!(lookup(chord(KeyCode::Char('q'), NONE), EMPTY), None);
        assert_eq!(lookup(chord(KeyCode::Char('z'), NONE), TYPED), None);
    }
}
