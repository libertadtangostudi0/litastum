//! The always-live command line at the bottom of the browser (Far
//! Manager-style — see `.claude/rules/litastum-stack.md` for why it's
//! append/backspace-only, no cursor movement).
//!
//! `insert_char`/`backspace`/`parse_cd_target` below are pure and
//! tested without a terminal; `handle_browsing_key`/`run_command_line`
//! are the impure key-handling/process-spawning half, moved here from
//! `main.rs` so this module owns everything about the command line
//! (mirrors `theme_menu.rs`/`menu.rs`, which each own their state *and*
//! key handling — `main.rs` stays a thin dispatcher).

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
use crate::{command, keymap};


/// Key handling in the browser: `Ctrl+P` opens the shell picker,
/// `Shift+F6` opens the rename prompt (both need the raw modifier,
/// which `keymap::resolve`'s table can't see since it only keys off
/// `KeyCode`), `Enter` with something typed runs it (`run_command_line`),
/// otherwise the fixed `keymap::resolve` table (arrows, Tab, F4/F9/F10,
/// and `Enter` on an *empty* command line — `EnterSelected`, unchanged)
/// takes over; anything that table doesn't bind — plain characters,
/// `Backspace`, `Esc` — edits the always-live command line at the
/// bottom of the browser. This is also why `q` no longer quits on its
/// own (`keymap.rs`) — a bare letter now types into the command line
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
        return command::execute(keymap::Command::RenameSelected, app);
    }

    if key.code == KeyCode::Enter && !app.command_line.is_empty() {
        return run_command_line(app, terminal);
    }

    if let Some(cmd) = keymap::resolve(key.code) {
        debug!(?cmd, "browsing command");
        return command::execute(cmd, app);
    }

    match key.code {
        KeyCode::Esc => app.command_line.clear(),
        KeyCode::Backspace => backspace(&mut app.command_line),
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            insert_char(&mut app.command_line, c);
        }
        _ => {}
    }

    Ok(())
}


/// Runs whatever's typed in `app.command_line`: `cd`-shaped input
/// changes the active panel's directory directly (`Panel::change_dir`
/// — a spawned shell's own `cd` could never affect our process, so
/// this has to be handled ourselves, same as Far Manager does it);
/// anything else suspends the TUI and hands the console to the
/// configured shell profile (`app.shell_profiles[app.active_shell]`),
/// inheriting stdio so interactive programs (an editor, a REPL, ...)
/// work too, not just one-shot commands.
fn run_command_line(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    let input = app.command_line.trim().to_string();
    app.command_line.clear();
    if input.is_empty() {
        return Ok(());
    }

    if let Some(target) = parse_cd_target(&input) {
        debug!(target, "command line: cd");
        app.active_panel().change_dir(target)?;
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
