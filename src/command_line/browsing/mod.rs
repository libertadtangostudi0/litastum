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
mod tests;
