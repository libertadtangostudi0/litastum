use std::io::Stdout;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{prelude::CrosstermBackend, Terminal};
use tracing::debug;

use crate::app::{App, Mode, ShellMenu};
use crate::explorer::{execute, resolve, Command, DriveMenu, FindFileState};

use super::completion::complete;
use super::history::{suggest_history, CommandHistoryMenu};

mod hidden_console;
mod shell_exec;

pub use shell_exec::run_shell_command_lines;
pub(crate) use shell_exec::run_command_line;

use hidden_console::toggle_panels_hidden;

/// Key handling in the browser. Modifier chords come first (they need
/// the raw modifier; `keymap::resolve` only sees `KeyCode`), then the
/// command line's own selection/word keys, `Enter` to run, the history
/// suggestions, `Tab` completion, the fixed `keymap::resolve` table, and
/// finally editing the always-live command line. Bare `Left`/`Right`
/// stay panel navigation. Order and scope cuts:
/// `.claude/rules/litastum-command-line.md`.
pub fn handle_browsing_key(app: &mut App, key: KeyEvent, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    debug!(?key, "browsing key");

    // Ctrl+O -- Far's show/hide panels (`hidden_console`).
    if key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return toggle_panels_hidden(app, terminal);
    }

    if key.code == KeyCode::Char('p') && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.mode = Mode::ShellMenu(ShellMenu { selected: app.active_shell });
        return Ok(());
    }

    // Ctrl+U -- Far's swap panels.
    if key.code == KeyCode::Char('u') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return execute(Command::SwapPanels, app);
    }

    // Shift+F6 -- rename (plain F6 is move).
    if key.code == KeyCode::F(6) && key.modifiers.contains(KeyModifiers::SHIFT) {
        return execute(Command::RenameSelected, app);
    }

    // Shift+Enter on an empty line -- a directory opens in the OS file
    // manager, a file in the built-in editor (`Command::OpenInFileManager`).
    if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::SHIFT) && app.command_line.is_empty() {
        return execute(Command::OpenInFileManager, app);
    }

    // Alt+F7 -- Find file.
    if key.code == KeyCode::F(7) && key.modifiers.contains(KeyModifiers::ALT) {
        app.mode = Mode::FindFile(FindFileState::new());
        return Ok(());
    }

    // Alt+F1/Alt+F2 -- change drive for the left/right panel (always
    // that panel, not the focused one, as in Far).
    if key.code == KeyCode::F(1) && key.modifiers.contains(KeyModifiers::ALT) {
        app.mode = Mode::ChangeDrive(DriveMenu::open(0));
        return Ok(());
    }
    if key.code == KeyCode::F(2) && key.modifiers.contains(KeyModifiers::ALT) {
        app.mode = Mode::ChangeDrive(DriveMenu::open(1));
        return Ok(());
    }

    // Alt+F8 -- command history.
    if key.code == KeyCode::F(8) && key.modifiers.contains(KeyModifiers::ALT) {
        app.mode = Mode::CommandHistory(CommandHistoryMenu::open());
        return Ok(());
    }

    // Alt+F5 -- Compare files (`compare_targets`). Must come before the
    // table, which maps any F5 to Copy. Silently does nothing if a path
    // can't be compared (e.g. a directory).
    if key.code == KeyCode::F(5) && key.modifiers.contains(KeyModifiers::ALT) {
        let Some((left_path, right_path)) = compare_targets(app) else {
            return Ok(());
        };
        let syntax_theme = app.syntax_theme.clone();
        if let Ok(state) = crate::compare::CompareState::open(left_path, right_path, syntax_theme, app.editor_keymap_mode) {
            app.mode = Mode::CompareFiles(state);
        }
        return Ok(());
    }

    // Ctrl+Shift+Left/Right -- word-wise selection in the command line
    // (`text_field`).
    if key.code == KeyCode::Left && key.modifiers.contains(KeyModifiers::SHIFT) && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.command_line.extend_selection_word_left();
        return Ok(());
    }
    if key.code == KeyCode::Right && key.modifiers.contains(KeyModifiers::SHIFT) && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.command_line.extend_selection_word_right();
        return Ok(());
    }

    // Shift+Left/Right -- character selection while something is typed;
    // on an empty line they fall through to panel marking below.
    if key.code == KeyCode::Left && key.modifiers.contains(KeyModifiers::SHIFT) && !key.modifiers.contains(KeyModifiers::CONTROL) && !app.command_line.is_empty() {
        app.command_line.extend_selection_left();
        return Ok(());
    }
    if key.code == KeyCode::Right && key.modifiers.contains(KeyModifiers::SHIFT) && !key.modifiers.contains(KeyModifiers::CONTROL) && !app.command_line.is_empty() {
        app.command_line.extend_selection_right();
        return Ok(());
    }

    // Ctrl+Left/Right -- move by a word and drop any selection (like a
    // real editor, not collapse to its nearer edge).
    if key.code == KeyCode::Left && key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::SHIFT) {
        app.command_line.move_word_left();
        return Ok(());
    }
    if key.code == KeyCode::Right && key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::SHIFT) {
        app.command_line.move_word_right();
        return Ok(());
    }

    // Shift+A / Shift+arrows -- mark entries (`panel/marks.rs`).
    // Shift+A only on an empty line, or a command could never start with
    // a capital letter.
    if key.modifiers.contains(KeyModifiers::SHIFT) && !key.modifiers.contains(KeyModifiers::CONTROL) {
        let mark_command = match key.code {
            KeyCode::Char('a' | 'A') if app.command_line.is_empty() => Some(Command::SelectAll),
            KeyCode::Up => Some(Command::MarkMoveUp),
            KeyCode::Down => Some(Command::MarkMoveDown),
            KeyCode::Left => Some(Command::MarkMoveLeft),
            KeyCode::Right => Some(Command::MarkMoveRight),
            _ => None,
        };
        if let Some(cmd) = mark_command {
            return execute(cmd, app);
        }
    }

    if key.code == KeyCode::Enter && !app.command_line.is_empty() {
        app.command_line_completion = None;
        return run_command_line(app, terminal);
    }

    // While history suggestions are showing, Up/Down/Tab work on them
    // (ahead of path completion). `Enter` always runs exactly what's
    // typed, so accepting a suggestion never runs something unexpected.
    let suggestions = suggest_history(&app.command_history, app.command_line.text());
    if !app.command_line.is_empty() && !suggestions.is_empty() && !app.command_line_suggestion_dismissed {
        match key.code {
            KeyCode::Up => {
                app.command_line_suggestion_selected = app.command_line_suggestion_selected.saturating_sub(1);
                return Ok(());
            }
            KeyCode::Down => {
                if app.command_line_suggestion_selected + 1 < suggestions.len() {
                    app.command_line_suggestion_selected += 1;
                }
                return Ok(());
            }
            KeyCode::Tab => {
                if let Some(&entry) = suggestions.get(app.command_line_suggestion_selected) {
                    app.command_line.set_text(entry);
                    app.command_line_completion = None;
                }
                app.command_line_suggestion_selected = 0;
                // See `App::command_line_suggestion_dismissed`.
                app.command_line_suggestion_dismissed = true;
                return Ok(());
            }
            _ => {}
        }
    }

    // Tab completes while something is typed; on an empty line it
    // switches panels (the table below).
    if key.code == KeyCode::Tab && !app.command_line.is_empty() {
        let cwd = app.active_panel().path.clone();
        let mut line = app.command_line.text().to_string();
        complete(&mut line, &cwd, &mut app.command_line_completion);
        app.command_line.set_text(line);
        return Ok(());
    }

    if let Some(cmd) = resolve(key.code) {
        debug!(?cmd, "browsing command");
        // A bound command ends any completion cycle and suggestion
        // browsing.
        app.command_line_completion = None;
        app.command_line_suggestion_selected = 0;
        // Focus moved to the panels -- drop the command-line selection.
        app.command_line.clear_selection();
        return execute(cmd, app);
    }

    match key.code {
        KeyCode::Esc => {
            app.command_line.clear();
            app.command_line_completion = None;
            app.command_line_suggestion_selected = 0;
            app.command_line_suggestion_dismissed = false;
        }
        KeyCode::Backspace => {
            app.command_line.backspace();
            app.command_line_completion = None;
            app.command_line_suggestion_selected = 0;
            app.command_line_suggestion_dismissed = false;
        }
        KeyCode::Delete => {
            app.command_line.delete_forward();
            app.command_line_completion = None;
            app.command_line_suggestion_selected = 0;
            app.command_line_suggestion_dismissed = false;
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.command_line.insert_char(c);
            app.command_line_completion = None;
            app.command_line_suggestion_selected = 0;
            app.command_line_suggestion_dismissed = false;
        }
        _ => {}
    }

    Ok(())
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
