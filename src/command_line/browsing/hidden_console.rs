use std::io::Stdout;

use color_eyre::eyre::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    style::Print,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::CrosstermBackend, Terminal};
use tracing::debug;

use crate::app::App;
use crate::command_line::history::{record_history, save_history};

use super::shell_exec::{append_command_line, drain_stale_input, parse_cd_target, print_themed, resolve_app_paths_command};

/// Whether an already-normalized `key` is `Ctrl+O` -- shared by the
/// hidden console's loop and `drain_stale_input`, so they can't drift.
pub(super) fn is_ctrl_o(key: crossterm::event::KeyEvent) -> bool {
    key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL)
}


/// `Ctrl+O` -- real Far Manager's own "show/hide panels" toggle.
/// Blocking: the main loop doesn't redraw until the panels come back.
///
/// While hidden it's a real command line, as in Far: typed characters
/// are echoed by hand (raw mode stays on), `Enter` runs the line
/// (`run_single_line_on_console`) and stays here; only `Ctrl+O` returns.
/// A `Ctrl+O` pressed while a child ran is honored as soon as it's
/// drained. Always ends with a newline, so the unterminated prompt isn't
/// continued by whatever prints next. History: docs/history/command-execution.md.
pub(in crate::command_line) fn toggle_panels_hidden(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    let mut input = String::new();
    print_prompt(app)?;

    loop {
        let Event::Key(key) = event::read()? else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        // See `keyboard_layout::normalize_ctrl_shortcut`'s own doc
        // comment -- this loop reads raw events directly, bypassing
        // `event_loop::keys::dispatch_key_event`'s own normalization entirely,
        // so `Ctrl+O` under a non-Latin layout needs it applied here too.
        let key = crate::keyboard_layout::normalize_ctrl_shortcut(key);
        if is_ctrl_o(key) {
            break;
        }

        match key.code {
            KeyCode::Enter => {
                execute!(std::io::stdout(), Print("\n"))?;
                let line = input.trim().to_string();
                input.clear();
                let mut ctrl_o_queued = false;
                if !line.is_empty() {
                    record_history(app, &line);
                    save_history(&app.command_history);
                    ctrl_o_queued = run_single_line_on_console(app, &line)?;
                }
                if ctrl_o_queued {
                    break;
                }
                print_prompt(app)?;
            }
            KeyCode::Backspace => {
                if input.pop().is_some() {
                    execute!(std::io::stdout(), Print("\u{8} \u{8}"))?;
                }
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                input.push(c);
                execute!(std::io::stdout(), Print(c))?;
            }
            _ => {}
        }
    }

    // Terminate the prompt, whichever way the loop exited.
    execute!(std::io::stdout(), Print("\n"))?;

    // Drop further queued Ctrl+O presses so they can't reopen this at once.
    let _ = drain_stale_input()?;

    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    terminal.clear()?;
    app.active_panel().reload()?;
    Ok(())
}


/// Echoes `"{cwd}> "` onto the real console, same prefix
/// `run_shell_command_lines` prints before each line it runs -- kept as
/// its own function since `toggle_panels_hidden`'s loop prints it again
/// after every command (the `cwd` may have just changed via `cd`).
fn print_prompt(app: &mut App) -> Result<()> {
    let cwd = app.active_panel().path.clone();
    print_themed(&app.theme, format_args!("{}> ", cwd.display()))
}


/// Runs one trimmed, non-empty line on the real console the hidden loop
/// is already on -- same `cd`/`cls`/shell-out handling as
/// `submit_command_line` + `apply_effect`, minus the alternate-screen
/// switching. Returns
/// whether a `Ctrl+O` was drained after a subprocess ran.
fn run_single_line_on_console(app: &mut App, line: &str) -> Result<bool> {
    if let Some(target) = parse_cd_target(line) {
        debug!(target, "hidden console: cd");
        app.active_panel().change_dir(target)?;
        return Ok(false);
    }

    if line == "cls" || line == "clear" {
        execute!(std::io::stdout(), crossterm::terminal::Clear(crossterm::terminal::ClearType::All), crossterm::cursor::MoveTo(0, 0))?;
        return Ok(false);
    }

    let profile = app.shell_profiles[app.active_shell].clone();
    let cwd = app.active_panel().path.clone();
    let mut command = std::process::Command::new(&profile.program);
    command.args(&profile.args_prefix);
    append_command_line(&mut command, &resolve_app_paths_command(line));

    disable_raw_mode()?;
    let status = command.current_dir(&cwd).status();
    enable_raw_mode()?;
    let ctrl_o_queued = drain_stale_input()?;

    match status {
        Ok(status) if !status.success() => {
            debug!(?status, "command exited non-zero");
        }
        Err(err) => print_themed(&app.theme, format_args!("failed to launch '{}': {err}\n", profile.program))?,
        Ok(_) => {}
    }
    Ok(ctrl_o_queued)
}


/// Only `is_ctrl_o` is testable here; the rest reads real console input.
#[cfg(test)]
mod tests {
    use crossterm::event::KeyEvent;

    use super::*;

    #[test]
    fn matches_ctrl_o_regardless_of_other_held_modifiers() {
        assert!(is_ctrl_o(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL)));
        assert!(is_ctrl_o(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL | KeyModifiers::SHIFT)));
    }

    #[test]
    fn rejects_o_without_control() {
        assert!(!is_ctrl_o(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE)));
        assert!(!is_ctrl_o(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::SHIFT)));
    }

    #[test]
    fn rejects_a_different_ctrl_letter() {
        assert!(!is_ctrl_o(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL)));
    }
}
