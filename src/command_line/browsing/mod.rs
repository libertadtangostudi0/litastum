use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tracing::debug;

use crate::app::{App, Mode};
use crate::explorer::{execute, DriveMenu, FindFileState};

use super::completion::complete;
use super::effect::Effect;
use super::history::{suggest_history, CommandHistoryMenu};

mod bindings;
mod hidden_console;
mod shell_exec;

pub(super) use hidden_console::toggle_panels_hidden;
pub(super) use shell_exec::run_shell_command_lines;
pub(crate) use shell_exec::submit_command_line;

use bindings::{BrowserAction, LineState};

/// Key handling in the browser: the first matching row of
/// `bindings::BINDINGS` (chords, command-line selection, marking, the
/// typed line, panel navigation and the F-key row, line editing), else a
/// plain character types into the command line. Order and scope cuts:
/// `.claude/rules/litastum-command-line.md`.
pub fn handle_browsing_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    debug!(?key, "browsing key");

    let line = LineState {
        empty: app.command_line.is_empty(),
        suggestions_showing: suggestions_showing(app),
    };
    match bindings::lookup(key, line) {
        Some(action) => perform(app, action),
        None => {
            if let KeyCode::Char(c) = key.code {
                if !key.modifiers.contains(KeyModifiers::CONTROL) {
                    app.command_line.insert_char(c);
                    line_edited(app);
                }
            }
            Ok(Effect::None)
        }
    }
}


/// Whether the history suggestions overlay is up (`ui::draw` shows it
/// under the same condition).
fn suggestions_showing(app: &App) -> bool {
    !app.command_line.is_empty() && !app.command_line_suggestion_dismissed && !suggest_history(&app.command_history, app.command_line.text()).is_empty()
}


/// Any edit ends a completion cycle and suggestion browsing, and lets
/// the suggestions show again.
fn line_edited(app: &mut App) {
    app.command_line_completion = None;
    app.command_line_suggestion_selected = 0;
    app.command_line_suggestion_dismissed = false;
}


fn perform(app: &mut App, action: BrowserAction) -> Result<Effect> {
    debug!(?action, "browsing action");
    match action {
        BrowserAction::Command(command) => execute(command, app)?,
        BrowserAction::Navigate(command) => {
            app.command_line_completion = None;
            app.command_line_suggestion_selected = 0;
            app.command_line.clear_selection();
            execute(command, app)?;
        }
        BrowserAction::ToggleHiddenPanels => return Ok(Effect::ToggleHiddenPanels),
        BrowserAction::OpenShellMenu => app.mode = Mode::ShellMenu(super::open_shell_menu(app)),
        BrowserAction::OpenFindFile => app.mode = Mode::FindFile(FindFileState::new()),
        BrowserAction::OpenDriveMenu(panel) => app.mode = Mode::ChangeDrive(DriveMenu::open(panel)),
        BrowserAction::OpenHistory => app.mode = Mode::CommandHistory(CommandHistoryMenu::open()),
        BrowserAction::CompareFiles => open_compare(app),
        BrowserAction::SelectWordLeft => app.command_line.extend_selection_word_left(),
        BrowserAction::SelectWordRight => app.command_line.extend_selection_word_right(),
        BrowserAction::SelectLeft => app.command_line.extend_selection_left(),
        BrowserAction::SelectRight => app.command_line.extend_selection_right(),
        BrowserAction::WordLeft => app.command_line.move_word_left(),
        BrowserAction::WordRight => app.command_line.move_word_right(),
        BrowserAction::Submit => {
            app.command_line_completion = None;
            return submit_command_line(app);
        }
        BrowserAction::SuggestionUp => app.command_line_suggestion_selected = app.command_line_suggestion_selected.saturating_sub(1),
        BrowserAction::SuggestionDown => {
            let count = suggest_history(&app.command_history, app.command_line.text()).len();
            crate::list_cursor::move_down(&mut app.command_line_suggestion_selected, count);
        }
        BrowserAction::AcceptSuggestion => {
            let entry = suggest_history(&app.command_history, app.command_line.text()).get(app.command_line_suggestion_selected).map(|entry| entry.to_string());
            if let Some(entry) = entry {
                app.command_line.set_text(entry);
                app.command_line_completion = None;
            }
            app.command_line_suggestion_selected = 0;
            // See `App::command_line_suggestion_dismissed`.
            app.command_line_suggestion_dismissed = true;
        }
        BrowserAction::Complete => {
            let cwd = app.active_panel().path.clone();
            let mut line = app.command_line.text().to_string();
            complete(&mut line, &cwd, &mut app.command_line_completion);
            app.command_line.set_text(line);
        }
        BrowserAction::ClearLine => {
            app.command_line.clear();
            line_edited(app);
        }
        BrowserAction::Backspace => {
            app.command_line.backspace();
            line_edited(app);
        }
        BrowserAction::DeleteForward => {
            app.command_line.delete_forward();
            line_edited(app);
        }
    }
    Ok(Effect::None)
}


/// `Alt+F5`: opens Compare on `compare_targets`; silently does nothing
/// if a path can't be compared (e.g. a directory).
fn open_compare(app: &mut App) {
    let Some((left_path, right_path)) = compare_targets(app) else {
        return;
    };
    let syntax_theme = app.syntax_theme.clone();
    if let Ok(state) = crate::compare::CompareState::open(left_path, right_path, syntax_theme, app.editor_keymap_mode) {
        app.mode = Mode::CompareFiles(state);
    }
}


/// Which two files `Alt+F5` compares: exactly two marked entries in the
/// active panel if so, otherwise each panel's cursor file (active panel
/// on the left). Any other marked count falls back too -- there's no
/// sensible third pane. `None` if a panel has nothing selected.
fn compare_targets(app: &App) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let marked_in_active_panel: Vec<std::path::PathBuf> = {
        let panel = &app.panels[app.active];
        panel.marked_entries().into_iter().map(|entry| panel.path.join(&entry.name)).collect()
    };
    if let [left, right] = marked_in_active_panel.as_slice() {
        return Some((left.clone(), right.clone()));
    }

    let left_path = app.panels[app.active].selected_path()?;
    let right_path = app.panels[1 - app.active].selected_path()?;
    Some((left_path, right_path))
}


#[cfg(test)]
mod compare_targets_tests {
    use super::compare_targets;
    use crate::test_support::{test_app, unique_scratch_dir};

    #[test]
    fn falls_back_to_the_two_panel_convention_when_nothing_is_marked() {
        let dir = unique_scratch_dir("compare-targets");
        std::fs::write(dir.join("only.txt"), "x").unwrap();
        let mut app = test_app(dir.clone());
        app.panels[0].move_down(); // off ".." and onto "only.txt", in both panels
        app.panels[1].move_down();

        let (left, right) = compare_targets(&app).expect("both panels have a selected file");
        assert_eq!(left, dir.join("only.txt"));
        assert_eq!(right, dir.join("only.txt"), "both panels start on the same directory/selection");
    }

    #[test]
    fn compares_two_marked_entries_in_the_active_panel_instead() {
        let dir = unique_scratch_dir("compare-targets");
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        std::fs::write(dir.join("b.txt"), "b").unwrap();
        let mut app = test_app(dir.clone());
        app.panels[app.active].select_all();

        let (left, right) = compare_targets(&app).expect("exactly two marked entries");
        assert_eq!(left, dir.join("a.txt"));
        assert_eq!(right, dir.join("b.txt"));
    }

    #[test]
    fn three_marked_entries_falls_back_to_the_two_panel_convention() {
        let dir = unique_scratch_dir("compare-targets");
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        std::fs::write(dir.join("b.txt"), "b").unwrap();
        std::fs::write(dir.join("c.txt"), "c").unwrap();
        let mut app = test_app(dir.clone());
        app.panels[app.active].select_all();

        let (left, _right) = compare_targets(&app).expect("both panels have a selected file");
        assert_eq!(left, app.panels[app.active].selected_path().unwrap(), "should have fallen back to the cursor's own selection, not picked two of the three marked entries");
    }
}


/// The browser's key routing -- testable now that terminal work comes
/// back as an `Effect` instead of being done in place.
#[cfg(test)]
mod handle_browsing_key_tests {
    use std::fs;

    use crossterm::event::KeyCode;

    use super::handle_browsing_key;
    use crate::app::App;
    use crate::command_line::Effect;
    use crate::test_support::{ctrl_key, key, shift_key, test_app, unique_scratch_dir};

    fn typed(line: &str) -> App {
        let mut app = test_app(unique_scratch_dir("browsing-keys"));
        app.command_line.set_text(line);
        app
    }

    #[test]
    fn ctrl_o_asks_for_the_hidden_console() {
        let mut app = typed("");
        assert_eq!(handle_browsing_key(&mut app, ctrl_key('o')).unwrap(), Effect::ToggleHiddenPanels);
    }

    #[test]
    fn enter_hands_the_typed_line_to_the_shell() {
        let mut app = typed("echo hi");

        let effect = handle_browsing_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert_eq!(effect, Effect::RunShell(vec!["echo hi".to_string()]));
        assert!(app.command_line.is_empty());
        assert_eq!(app.command_history.last().map(String::as_str), Some("echo hi"));
    }

    #[test]
    fn enter_on_cls_asks_for_a_repaint_not_a_shell() {
        let mut app = typed("cls");
        assert_eq!(handle_browsing_key(&mut app, key(KeyCode::Enter)).unwrap(), Effect::ClearScreen);
    }

    #[test]
    fn enter_on_cd_moves_the_panel_without_a_shell() {
        let mut app = typed("cd sub");
        let target = app.panels[app.active].path.join("sub");
        fs::create_dir_all(&target).unwrap();

        assert_eq!(handle_browsing_key(&mut app, key(KeyCode::Enter)).unwrap(), Effect::None);
        assert_eq!(app.panels[app.active].path, target);
    }

    #[test]
    fn tab_switches_panels_on_an_empty_line_but_completes_while_typing() {
        let mut app = typed("");
        handle_browsing_key(&mut app, key(KeyCode::Tab)).unwrap();
        assert_eq!(app.active, 1, "Tab on an empty line switches panels");

        let mut app = typed("cd su");
        fs::create_dir_all(app.panels[app.active].path.join("sub")).unwrap();
        handle_browsing_key(&mut app, key(KeyCode::Tab)).unwrap();
        assert_eq!(app.active, 0, "Tab while typing must not switch panels");
        assert!(app.command_line.text().starts_with("cd sub"), "completed: {:?}", app.command_line.text());
    }

    #[test]
    fn shift_a_marks_everything_only_on_an_empty_line() {
        let dir = unique_scratch_dir("browsing-keys-shift-a");
        fs::write(dir.join("a.txt"), "a").unwrap();
        let mut app = test_app(dir);

        handle_browsing_key(&mut app, shift_key(KeyCode::Char('A'))).unwrap();
        assert!(!app.panels[app.active].marked_entries().is_empty(), "empty line: Shift+A marks all");
        assert!(app.command_line.is_empty());

        app.command_line.set_text("x");
        handle_browsing_key(&mut app, shift_key(KeyCode::Char('A'))).unwrap();
        assert_eq!(app.command_line.text(), "xA", "while typing, Shift+A is just a capital letter");
    }

    #[test]
    fn a_plain_letter_types_instead_of_acting_as_a_shortcut() {
        let mut app = typed("");
        handle_browsing_key(&mut app, key(KeyCode::Char('q'))).unwrap();
        assert_eq!(app.command_line.text(), "q");
        assert!(!app.should_quit, "only F10 quits; a bare q is the start of a command");
    }

    #[test]
    fn shift_left_selects_in_the_typed_line() {
        let mut app = typed("abc");
        handle_browsing_key(&mut app, shift_key(KeyCode::Left)).unwrap();
        assert_eq!(app.command_line.selection(), Some((2, 3)));
    }
}
