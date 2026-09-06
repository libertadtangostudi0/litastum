use std::io::Stdout;

use color_eyre::eyre::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::CrosstermBackend, Terminal};
use tracing::debug;

use crate::app::{App, Mode, ShellMenu};
use crate::explorer::{execute, resolve, Command, DriveMenu, FindFileState};

use super::completion::complete;
use super::history::{record_history, save_history, suggest_history, CommandHistoryMenu};

/// Key handling in the browser: `Ctrl+P` opens the shell picker,
/// `Shift+F6` opens the rename prompt, `Alt+F1`/`Alt+F2`/`Alt+F7`/
/// `Alt+F8` open their own popups (all need the raw modifier, which
/// `keymap::resolve`'s table can't see since it only keys off
/// `KeyCode`), `Enter` with something typed runs it
/// (`run_command_line`). While an auto-popping history-suggestion list
/// is actually showing (`suggest_history` found at least one match),
/// `Up`/`Down` move within it and `Tab` accepts the highlighted entry
/// into the command line instead of their usual meaning (panel
/// navigation / path completion) — see the dedicated check below for
/// why `Enter` is deliberately *not* part of that. Otherwise `Tab`
/// completes a path while something's typed, then the fixed
/// `keymap::resolve` table (arrows, Tab, F4/F9/F10, and `Enter` on an
/// *empty* command line — `EnterSelected`, unchanged) takes over;
/// anything that table doesn't bind — plain characters, `Backspace`,
/// `Esc` — edits the always-live command line at the bottom of the
/// browser. This is also why `q` no longer quits on its own
/// (`keymap.rs`) — a bare letter now types into the command line
/// like any other.
pub fn handle_browsing_key(app: &mut App, key: KeyEvent, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    debug!(?key, "browsing key");

    if key.code == KeyCode::Char('p') && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.mode = Mode::ShellMenu(ShellMenu { selected: app.active_shell });
        return Ok(());
    }

    // Shift+F6 (rename) vs plain F6 (move) only differ by modifier --
    // keymap::resolve's table keys off KeyCode alone, so this one has
    // to be special-cased ahead of it, same as Ctrl+P above.
    if key.code == KeyCode::F(6) && key.modifiers.contains(KeyModifiers::SHIFT) {
        return execute(Command::RenameSelected, app);
    }

    // Alt+F7 -- real Far Manager's own global shortcut for "Find file",
    // previously only reachable through F9 -> Commands -> Find file.
    // Same reason as Shift+F6 above: needs the raw modifier.
    if key.code == KeyCode::F(7) && key.modifiers.contains(KeyModifiers::ALT) {
        app.mode = Mode::FindFile(FindFileState::new());
        return Ok(());
    }

    // Alt+F1/Alt+F2 -- real Far Manager's own per-panel "change drive"
    // popup. Always the left/right panel respectively, not whichever
    // one currently has focus (`DriveMenu::open`'s `target_panel`),
    // matching real Far -- same raw-modifier reasoning as above.
    if key.code == KeyCode::F(1) && key.modifiers.contains(KeyModifiers::ALT) {
        app.mode = Mode::ChangeDrive(DriveMenu::open(0));
        return Ok(());
    }
    if key.code == KeyCode::F(2) && key.modifiers.contains(KeyModifiers::ALT) {
        app.mode = Mode::ChangeDrive(DriveMenu::open(1));
        return Ok(());
    }

    // Alt+F8 -- real Far Manager's own global shortcut for "History",
    // previously only reachable through F9 -> Commands -> History.
    // Same raw-modifier reasoning as above.
    if key.code == KeyCode::F(8) && key.modifiers.contains(KeyModifiers::ALT) {
        app.mode = Mode::CommandHistory(CommandHistoryMenu::open());
        return Ok(());
    }

    if key.code == KeyCode::Enter && !app.command_line.is_empty() {
        app.command_line_completion = None;
        return run_command_line(app, terminal);
    }

    // Auto-popping history suggestions (`ui::draw_history_suggestions`)
    // claim Up/Down/Tab while they're actually showing -- i.e. only
    // once there's a non-empty command line with at least one deduped
    // substring match (`suggest_history`), same "only while something
    // matters" guard the older Tab-path-completion check below already
    // uses. Checked ahead of that Tab check so a showing suggestion
    // list wins the key over path completion; falls through untouched
    // to normal panel navigation / path completion whenever there's
    // nothing to suggest, so this never steals arrows or Tab
    // otherwise. `Enter` is deliberately left alone here -- it always
    // just runs whatever's literally typed (`run_command_line`,
    // below), suggestion showing or not, so accepting one never
    // surprises you into running something you didn't type.
    let suggestions = suggest_history(&app.command_history, &app.command_line);
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
                    app.command_line = entry.to_string();
                    app.command_line_completion = None;
                }
                app.command_line_suggestion_selected = 0;
                // The just-accepted command line is always itself a
                // substring match of the entry it came from, so the
                // same list would otherwise reappear unchanged on the
                // very next frame -- suppress it until an actual edit
                // (insert_char/backspace/Esc, below) clears this again.
                app.command_line_suggestion_dismissed = true;
                return Ok(());
            }
            _ => {}
        }
    }

    // Tab completes the command line's typed text (below) while
    // there's something to complete; only falls through to
    // `keymap::resolve`'s Tab-as-ToggleActive binding once the line is
    // empty. Without this check, Tab always switched panels, even
    // mid-command -- exactly backwards from every shell's own
    // convention for the key.
    if key.code == KeyCode::Tab && !app.command_line.is_empty() {
        let cwd = app.active_panel().path.clone();
        complete(&mut app.command_line, &cwd, &mut app.command_line_completion);
        return Ok(());
    }

    if let Some(cmd) = resolve(key.code) {
        debug!(?cmd, "browsing command");
        // Any bound command (including plain Tab on an empty line,
        // switching panels) leaves the command line untouched, so a
        // stale completion cycle wouldn't otherwise get cleared here --
        // but there's nothing left to cycle through once the mode
        // changes or the panel does, so drop it regardless.
        app.command_line_completion = None;
        app.command_line_suggestion_selected = 0;
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
            backspace(&mut app.command_line);
            app.command_line_completion = None;
            app.command_line_suggestion_selected = 0;
            app.command_line_suggestion_dismissed = false;
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            insert_char(&mut app.command_line, c);
            app.command_line_completion = None;
            app.command_line_suggestion_selected = 0;
            app.command_line_suggestion_dismissed = false;
        }
        _ => {}
    }

    Ok(())
}

/// Runs whatever's typed in `app.command_line`: `cd`-shaped input
/// changes the active panel's directory directly (`Panel::change_dir`
/// — a spawned shell's own `cd` could never affect our process, so
/// this has to be handled ourselves, same as Far Manager does it);
/// `cls`/`clear` repaint the TUI directly (below) rather than actually
/// shelling out; anything else suspends the TUI and hands the console
/// to the configured shell profile (`app.shell_profiles[app.active_shell]`),
/// inheriting stdio so interactive programs (an editor, a REPL, ...)
/// work too, not just one-shot commands.
fn run_command_line(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    let input = app.command_line.trim().to_string();
    app.command_line.clear();
    if input.is_empty() {
        return Ok(());
    }
    record_history(app, &input);
    save_history(&app.command_history);

    if let Some(target) = parse_cd_target(&input) {
        debug!(target, "command line: cd");
        app.active_panel().change_dir(target)?;
        return Ok(());
    }

    // A screen-clear command's entire job is leaving nothing on
    // screen -- shelling out to a real `cls`/`clear` did exactly that,
    // including wiping the "{cwd}> cls" prompt line printed just below
    // for every other command, and then the "Press any key to
    // continue..." pause (which exists so real command *output* isn't
    // lost the instant the panels redraw over it) had nothing left to
    // protect -- just a stray message floating on an otherwise blank
    // screen. Reported as a confusing/broken-looking screen; fixed by
    // never leaving the TUI for these two at all, matching what the
    // command is actually trying to accomplish (a repaint) far more
    // directly than round-tripping through a real subprocess.
    if input == "cls" || input == "clear" {
        debug!("command line: clear screen (handled directly, no subprocess)");
        terminal.clear()?;
        app.active_panel().reload()?;
        return Ok(());
    }

    let profile = app.shell_profiles[app.active_shell].clone();
    let cwd = app.active_panel().path.clone();
    debug!(shell = profile.name, %input, cwd = %cwd.display(), "command line: running");

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    println!("{}> {input}", cwd.display());
    let status = std::process::Command::new(&profile.program)
        .args(&profile.args_prefix)
        .arg(&input)
        .current_dir(&cwd)
        .status();
    match status {
        Ok(status) if !status.success() => {
            debug!(?status, "command exited non-zero");
        }
        Err(err) => println!("failed to launch '{}': {err}", profile.program),
        Ok(_) => {}
    }
    println!("\nPress any key to continue...");

    // Wait for one real keypress before redrawing -- otherwise output
    // that scrolled by fast is gone the instant the panels repaint.
    loop {
        if let Event::Key(k) = event::read()? {
            if k.kind == KeyEventKind::Press {
                break;
            }
        }
    }

    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    terminal.clear()?;

    app.active_panel().reload()?;
    Ok(())
}

/// Appends `c` to the typed command.
pub fn insert_char(line: &mut String, c: char) {
    line.push(c);
}

/// Removes the last character, if any. A no-op on an empty line.
pub fn backspace(line: &mut String) {
    line.pop();
}

/// If `input` is a `cd` command, returns its argument (trimmed) — or
/// `None` for the argument-less `"cd"` (a no-op, not "go home"; see
/// the plan doc), and `None` for anything that isn't `cd` at all
/// (including a different command that merely starts with "cd", like
/// `"cdw"` — checked via a word boundary, not a bare prefix).
pub fn parse_cd_target(input: &str) -> Option<&str> {
    let rest = input.strip_prefix("cd")?;
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None; // e.g. "cdw ..", not "cd"
    }
    let target = rest.trim();
    if target.is_empty() {
        None
    } else {
        Some(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod line_editing_tests {
        use super::*;

        #[test]
        fn insert_char_appends() {
            let mut line = String::from("di");
            insert_char(&mut line, 'r');
            assert_eq!(line, "dir");
        }

        #[test]
        fn backspace_removes_last_char() {
            let mut line = String::from("dir");
            backspace(&mut line);
            assert_eq!(line, "di");
        }

        #[test]
        fn backspace_on_empty_line_is_a_noop() {
            let mut line = String::new();
            backspace(&mut line);
            assert_eq!(line, "");
        }
    }

    mod parse_cd_target_tests {
        use super::*;

        #[test]
        fn parse_cd_target_extracts_the_argument() {
            assert_eq!(parse_cd_target("cd .."), Some(".."));
            assert_eq!(parse_cd_target("cd src"), Some("src"));
            assert_eq!(parse_cd_target("cd   spaced   "), Some("spaced"));
        }

        #[test]
        fn parse_cd_target_bare_cd_is_none() {
            assert_eq!(parse_cd_target("cd"), None);
            assert_eq!(parse_cd_target("cd   "), None);
        }

        #[test]
        fn parse_cd_target_rejects_other_commands() {
            assert_eq!(parse_cd_target("cdw --version"), None);
            assert_eq!(parse_cd_target("cargo build"), None);
            assert_eq!(parse_cd_target(""), None);
        }

        #[test]
        fn parse_cd_target_is_case_sensitive() {
            // Matches cmd.exe/sh convention (cd is lowercase); CD/Cd are
            // handled fine by cmd.exe itself if shelled out instead.
            assert_eq!(parse_cd_target("CD src"), None);
        }
    }
}
