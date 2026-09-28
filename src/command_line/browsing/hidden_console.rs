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

/// Whether an already-normalized `key` is the raw `Ctrl+O` chord --
/// pulled out into its own function so `toggle_panels_hidden`'s own
/// live event loop and `drain_stale_input`'s queued-event scan can't
/// drift out of sync on what counts as "the user asked to leave this
/// mode," and so the check has a name testable on its own without a
/// real console.
pub(super) fn is_ctrl_o(key: crossterm::event::KeyEvent) -> bool {
    key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL)
}


/// `Ctrl+O` -- real Far Manager's own "show/hide panels" toggle.
/// Blocking, the same shape as `run_shell_command_lines` itself: this
/// simply doesn't return control to the main loop's own `terminal.draw()`
/// call until the panels should reappear.
///
/// **Also a real command line now, not just a viewer** -- requested
/// directly, since real Far Manager's own hidden-panels view lets you
/// keep typing commands right there rather than only being able to look
/// and then bring the panels straight back. Typed characters are echoed
/// (raw mode suppresses the console's own echo, so this loop has to do
/// it manually) and `Enter` runs the line through the same `cd`/`cls`/
/// shell-out handling `run_command_line` uses -- but, unlike that path,
/// stays right here afterward instead of restoring the panels: the
/// point of this mode is to keep working directly against the real
/// console, and forcing a return to the TUI after every command would
/// defeat that. Only `Ctrl+O` itself brings the panels back.
///
/// Raw mode stays enabled for the interactive typing loop itself (same
/// as the rest of the app -- an OS-level line discipline would swallow
/// keystrokes until its own `Enter`, and crossterm needs raw mode to
/// deliver them one at a time for this loop to echo itself). It's
/// toggled off only for the moment a real subprocess actually runs
/// (`run_single_line_on_console`), same bracketing
/// `run_shell_command_lines` uses, so an interactive child (an editor,
/// a REPL, ...) still gets normal line-buffered input.
///
/// **Two bugs reported together against a real, slow-running `svn`
/// command** (a merge/update reporting conflicts, `"Summary of
/// conflicts: Text conflicts: 2"`):
///
/// 1. Pressing `Ctrl+O` to return to the panels sometimes silently did
///    nothing. Root cause: while `run_single_line_on_console`'s own
///    subprocess has the console (raw mode off), a `Ctrl+O` pressed
///    meanwhile doesn't reach this loop's own `event::read()` at all --
///    it just queues up at the OS console level like any other
///    keystroke, same mechanism `drain_stale_input`'s own doc comment
///    already covers for a stray `Enter`. But unlike a stray `Enter`
///    (safe to discard -- it only ever means "run an empty command," a
///    no-op either way), throwing away a genuine `Ctrl+O` the same way
///    discards the user's actual intent to leave this mode.
///    `run_single_line_on_console` now reports whether it saw one, and
///    this loop honors it immediately below (`ctrl_o_queued`) instead
///    of printing another prompt and waiting.
///
/// 2. The `"{cwd}> "` prompt itself printed twice in a row on what
///    looked like one line, with nothing typed in between. **Fully
///    deterministic, not a flaky double-press** -- `print_prompt`
///    deliberately never prints a trailing newline (the whole point is
///    leaving the cursor right after it, ready for the user's own
///    typing on that same line). Leaving this mode via `Ctrl+O`
///    *without ever pressing `Enter`* -- an entirely ordinary "peek at
///    the console, then close it" use of this toggle, no repeated
///    keypress required -- returns to the panels with that `"path> "`
///    still dangling, unterminated, on the real console buffer. The
///    very next thing written to that same real buffer -- another
///    `Ctrl+O` peek, or the regular always-live command line's own next
///    command via `run_shell_command_lines`'s `"{cwd}> {line}\n"` echo
///    -- picks up writing right where that dangling prompt left off,
///    with no newline separating them: exactly `"path> path> svn st"`,
///    which a real terminal then soft-wraps into what looks like two
///    lines. Fixed below: this function always emits one trailing
///    newline before leaving, whichever way it exits, so the real
///    buffer's cursor is never left mid-line for whatever gets printed
///    there next.
pub(super) fn toggle_panels_hidden(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
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

    // See this function's own doc comment, bug 2 -- whichever way the
    // loop above exited, `print_prompt`'s own "{cwd}> " may still be
    // sitting unterminated on the real console (never printed a
    // trailing newline, by design, so typed input lands on the same
    // line). Terminating it here, unconditionally, guarantees the next
    // thing written to this same real buffer -- another peek, or the
    // regular command line's own next echoed prompt -- always starts
    // on a fresh line, rather than concatenating onto this one.
    execute!(std::io::stdout(), Print("\n"))?;

    // Belt-and-suspenders alongside the newline above: also drop any
    // further queued Ctrl+O so a genuinely repeated keypress can't
    // immediately re-trigger this same function from `event_loop`'s own
    // dispatch with nothing else having happened in between.
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


/// Runs one already-trimmed, non-empty line directly against the real
/// console `toggle_panels_hidden`'s loop is already sitting on -- no
/// alternate-screen or panel-redraw dance around it, since that loop
/// never left the real console in the first place. Shares `cd`/`cls`
/// handling and the actual shell-out with `run_command_line`, just
/// without that function's own leave/re-enter-alternate-screen and
/// "press any key" pause, which only make sense when returning to the
/// TUI is the point.
///
/// Returns whether a `Ctrl+O` was seen among the input drained after a
/// real subprocess ran -- `false` unconditionally for the `cd`/`cls`
/// branches, which never suspend raw mode at all and so can't have
/// left anything queued behind them. See `toggle_panels_hidden`'s own
/// doc comment for why the caller can't just ignore this the way it
/// ignores every *other* kind of drained input.
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


/// `is_ctrl_o` is the one piece of the two-bugs-in-one fix
/// (`toggle_panels_hidden`'s own doc comment has the full story)
/// that doesn't need a real console to exercise -- `drain_stale_input`
/// and `toggle_panels_hidden` themselves read genuine OS input events
/// and have no test coverage for the same reason
/// `event_loop::handle_event`/`command_line::browsing::handle_browsing_key`
/// don't either.
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
