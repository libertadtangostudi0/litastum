use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tracing::debug;

use crate::app::{App, Mode, Overlay};
use crate::explorer::{execute, Command, DriveMenu, FindFileState};

use super::completion::complete;
use super::effect::Effect;
use super::history::{forget_history, CommandHistoryMenu};
use super::suggestions::{suggestions, Suggestion};

mod bindings;
mod hidden_console;
mod live_command;
mod panel_path;
mod shell_exec;
mod type_ahead;

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
    if app.panel_path_edit.is_some() {
        panel_path::key(app, key);
        return Ok(Effect::None);
    }
    if app.panels_hidden {
        return hidden_console::console_key(app, key);
    }

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


/// Whether the suggestions overlay is up (`ui::draw` shows it under the
/// same condition).
fn suggestions_showing(app: &App) -> bool {
    !app.command_line.is_empty() && !app.command_line_suggestion_dismissed && !suggestions(app).is_empty()
}


/// A mouse event in the browser: the user screen's while the panels are
/// hidden (the wheel scrolls), else the panels' (a click on a title).
pub fn handle_browsing_mouse(app: &mut App, mouse: crossterm::event::MouseEvent) {
    if app.panels_hidden {
        hidden_console::mouse(app, mouse);
    } else {
        panel_path::mouse(app, mouse);
    }
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
        BrowserAction::ToggleHiddenPanels => app.panels_hidden = true,
        BrowserAction::OpenShellMenu => app.overlay = Some(Overlay::ShellMenu(super::open_shell_menu(app))),
        BrowserAction::OpenFindFile => app.overlay = Some(Overlay::FindFile(FindFileState::new())),
        BrowserAction::OpenDriveMenu(panel) => app.overlay = Some(Overlay::ChangeDrive(DriveMenu::open(panel))),
        BrowserAction::OpenHistory => app.overlay = Some(Overlay::CommandHistory(CommandHistoryMenu::open())),
        BrowserAction::CompareFiles => open_compare(app),
        BrowserAction::EditPanelPath => panel_path::start(app),
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
            let count = suggestions(app).len();
            crate::list_cursor::move_down(&mut app.command_line_suggestion_selected, count);
        }
        BrowserAction::AcceptSuggestion => {
            let line = suggestions(app).get(app.command_line_suggestion_selected).map(|suggestion| suggestion.accepted(app.command_line.text()));
            if let Some(line) = line {
                app.command_line.set_text(line);
                app.command_line_completion = None;
            }
            app.command_line_suggestion_selected = 0;
            // See `App::command_line_suggestion_dismissed`.
            app.command_line_suggestion_dismissed = true;
        }
        BrowserAction::DeleteSuggestion => {
            // Only a history entry: a panel name is a file, never deleted here.
            if let Some(Suggestion::History(entry)) = suggestions(app).get(app.command_line_suggestion_selected).cloned() {
                forget_history(app, &entry);
            }
            let count = suggestions(app).len();
            app.command_line_suggestion_selected = app.command_line_suggestion_selected.min(count.saturating_sub(1));
        }
        BrowserAction::EditSuggestion => {
            if let Some(Suggestion::File { name, is_dir: false }) = suggestions(app).get(app.command_line_suggestion_selected).cloned() {
                // As F4 in the panel path field's list: the panel's cursor
                // moves onto the file, then the editor opens it.
                let panel = app.active_panel();
                if let Some(index) = panel.entries.iter().position(|entry| entry.name == name) {
                    panel.selected = index;
                }
                app.command_line_suggestion_selected = 0;
                app.command_line_suggestion_dismissed = true;
            }
            execute(Command::EditSelected, app)?;
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
        BrowserAction::CopySelection => app.command_line.copy_selection(),
        BrowserAction::CutSelection => {
            if app.command_line.cut_selection() {
                line_edited(app);
            }
        }
    }
    Ok(Effect::None)
}


/// `Alt+F5`: the conflict resolver on an SVN conflict's four marked
/// files (`conflict::detect`), otherwise Compare on `compare_targets`;
/// Compare silently does nothing if a path can't be compared (e.g. a
/// directory).
fn open_compare(app: &mut App) {
    if let Some(files) = crate::conflict::detect(&marked_paths(app)) {
        crate::conflict::open_resolver(app, files);
        return;
    }
    let Some((left_path, right_path)) = compare_targets(app) else {
        return;
    };
    let syntax_theme = app.syntax_theme.clone();
    if let Ok(state) = crate::compare::CompareState::open(left_path, right_path, syntax_theme, app.settings.editor_keymap_mode) {
        app.mode = Mode::CompareFiles(state);
    }
}


/// Which two files `Alt+F5` compares: exactly two marked entries in the
/// active panel if so, otherwise each panel's cursor file (active panel
/// on the left). Any other marked count falls back too -- four make a
/// conflict only when `conflict::detect` says so. `None` if a panel has
/// nothing selected.
fn compare_targets(app: &App) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    if let [left, right] = marked_paths(app).as_slice() {
        return Some((left.clone(), right.clone()));
    }

    let left_path = app.panels[app.active].selected_path()?;
    let right_path = app.panels[1 - app.active].selected_path()?;
    Some((left_path, right_path))
}


fn marked_paths(app: &App) -> Vec<std::path::PathBuf> {
    let panel = &app.panels[app.active];
    panel.marked_entries().into_iter().map(|entry| panel.path.join(&entry.name)).collect()
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
    fn four_marked_conflict_files_open_the_resolver() {
        let dir = unique_scratch_dir("compare-targets");
        crate::conflict::state_tests::write_conflict_files(&dir);
        let mut app = test_app(dir);
        app.panels[app.active].select_all();

        super::open_compare(&mut app);

        let crate::app::Mode::ResolveConflict(state) = &app.mode else { panic!("expected the conflict resolver") };
        assert!(state.result.text().contains("<<<<<<<"));
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

    fn select_last_chars(app: &mut App, count: usize) {
        for _ in 0..count {
            handle_browsing_key(app, shift_key(KeyCode::Left)).unwrap();
        }
    }

    /// Reported: a selected part of a typed command couldn't be copied.
    #[test]
    fn ctrl_c_copies_the_selected_part_of_the_line() {
        let mut app = typed("svn merge -c 172418,172507 --accept postpone");
        select_last_chars(&mut app, 8);

        handle_browsing_key(&mut app, ctrl_key('c')).unwrap();

        assert_eq!(crate::text_field::clipboard::get().as_deref(), Some("postpone"));
        assert_eq!(app.command_line.text(), "svn merge -c 172418,172507 --accept postpone", "copying leaves the line alone");
    }

    #[test]
    fn ctrl_insert_copies_too() {
        let mut app = typed("cd src");
        select_last_chars(&mut app, 3);

        handle_browsing_key(&mut app, crossterm::event::KeyEvent::new(KeyCode::Insert, crossterm::event::KeyModifiers::CONTROL)).unwrap();

        assert_eq!(crate::text_field::clipboard::get().as_deref(), Some("src"));
    }

    #[test]
    fn ctrl_x_cuts_the_selected_part_of_the_line() {
        let mut app = typed("svn up trunk");
        select_last_chars(&mut app, 5);

        handle_browsing_key(&mut app, ctrl_key('x')).unwrap();

        assert_eq!(crate::text_field::clipboard::get().as_deref(), Some("trunk"));
        assert_eq!(app.command_line.text(), "svn up ");
    }

    #[test]
    fn ctrl_c_without_a_selection_copies_nothing_and_types_nothing() {
        let mut app = typed("dir");

        handle_browsing_key(&mut app, ctrl_key('c')).unwrap();

        assert_eq!(crate::text_field::clipboard::get(), None);
        assert_eq!(app.command_line.text(), "dir");
    }

    #[test]
    fn ctrl_o_hides_the_panels_and_brings_them_back() {
        let mut app = typed("");
        handle_browsing_key(&mut app, ctrl_key('o')).unwrap();
        assert!(app.panels_hidden);
        handle_browsing_key(&mut app, ctrl_key('o')).unwrap();
        assert!(!app.panels_hidden);
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

    /// Reported: a mistyped command kept being suggested, with no way to
    /// get rid of it.
    #[test]
    fn f8_forgets_the_highlighted_suggestion_and_deletes_no_file() {
        let dir = unique_scratch_dir("browsing-keys-f8");
        fs::write(dir.join("keep.txt"), "x").unwrap();
        let mut app = test_app(dir.clone());
        app.command_history = vec!["svn up".into(), "svn info".into(), "svn up".into(), "cargo build".into()];
        app.command_line.set_text("svn");
        handle_browsing_key(&mut app, key(KeyCode::Down)).unwrap(); // newest first: "svn up", then "svn info"

        handle_browsing_key(&mut app, key(KeyCode::F(8))).unwrap();

        assert_eq!(app.command_history, vec!["svn up".to_string(), "svn up".into(), "cargo build".into()]);
        assert_eq!(app.command_line.text(), "svn", "the typed line stays");
        assert_eq!(app.command_line_suggestion_selected, 0, "clamped to the one suggestion left");
        assert!(app.overlay.is_none(), "no delete confirmation");
        assert!(dir.join("keep.txt").exists());

        handle_browsing_key(&mut app, key(KeyCode::F(8))).unwrap();
        assert_eq!(app.command_history, vec!["cargo build".to_string()], "every copy of the command goes");
    }

    /// Reported: a file name couldn't be completed from the suggestions.
    #[test]
    fn tab_accepts_a_panel_name_into_the_typed_word_and_f8_leaves_files_alone() {
        let dir = unique_scratch_dir("browsing-keys-names");
        fs::write(dir.join("cmt_msg.txt"), "x").unwrap();
        let mut app = test_app(dir.clone());
        app.command_line.set_text("svn commit -F cm");

        handle_browsing_key(&mut app, key(KeyCode::F(8))).unwrap();
        assert!(dir.join("cmt_msg.txt").exists(), "F8 on a file suggestion deletes nothing");
        assert!(app.overlay.is_none());

        handle_browsing_key(&mut app, key(KeyCode::Tab)).unwrap();
        assert_eq!(app.command_line.text(), "svn commit -F cmt_msg.txt ");
    }

    /// Requested: F4 on a panel name in the suggestions moves the panel's
    /// cursor onto that file and opens it, as in the path field's list.
    #[test]
    fn f4_on_a_file_suggestion_selects_it_in_the_panel_and_opens_the_editor() {
        let dir = unique_scratch_dir("browsing-keys-f4");
        fs::write(dir.join("a.txt"), "a").unwrap();
        fs::write(dir.join("cmt_msg.txt"), "message\n").unwrap();
        let mut app = test_app(dir.clone());
        app.command_line.set_text("svn commit -F cm");

        handle_browsing_key(&mut app, key(KeyCode::F(4))).unwrap();

        assert_eq!(app.panels[app.active].current().map(|entry| entry.name.as_str()), Some("cmt_msg.txt"));
        let crate::app::Mode::Editing(editor) = &app.mode else { panic!("expected the editor") };
        assert_eq!(editor.path(), dir.join("cmt_msg.txt"));
        assert_eq!(app.command_line.text(), "svn commit -F cm", "the typed line stays");
    }

    /// Reported: with `cmt_msg.txt` also in the history, its row was a
    /// history entry and F4 didn't open the file.
    #[test]
    fn f4_opens_a_file_whose_name_is_also_in_the_history() {
        let dir = unique_scratch_dir("browsing-keys-f4");
        fs::write(dir.join("cmt_msg.txt"), "message
").unwrap();
        let mut app = test_app(dir.clone());
        app.command_history = vec!["cmt_msg.txt".into(), "svn commit -F cmt_msg.txt RFI14.1".into()];
        app.command_line.set_text("cmt");
        handle_browsing_key(&mut app, key(KeyCode::Down)).unwrap(); // newest history first, then the name

        handle_browsing_key(&mut app, key(KeyCode::F(4))).unwrap();

        let crate::app::Mode::Editing(editor) = &app.mode else { panic!("expected the editor") };
        assert_eq!(editor.path(), dir.join("cmt_msg.txt"));
    }

    #[test]
    fn shift_left_selects_in_the_typed_line() {
        let mut app = typed("abc");
        handle_browsing_key(&mut app, shift_key(KeyCode::Left)).unwrap();
        assert_eq!(app.command_line.selection(), Some((2, 3)));
    }
}
