use std::io::Stdout;

use color_eyre::eyre::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    style::{Color as CtColor, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::CrosstermBackend, Terminal};
use tracing::debug;

use crate::app::App;
use crate::command_line::history::{record_history, save_history};

/// Prints one line to the real (TUI-suspended) console using
/// `theme.text` on `theme.bg` -- as close as this project's "inherit
/// stdio, don't capture it" design (see
/// `.claude/rules/litastum-command-line.md`) can get to real Far
/// Manager's own `CommandLine.UserScreen` color group. This only
/// colors litastum's *own* printed lines (the echoed `"{cwd}> "`
/// prompt and the "Press any key..." pause) -- the shelled-out
/// command's own output is never touched, since it's real inherited
/// stdio, not something rendered through our own buffer the way Far's
/// full-screen text-mode architecture lets it recolor everything
/// (including a child process's output). Reproducing that would need
/// a PTY-based capture-and-recolor layer -- a much bigger redesign
/// than this project's current "suspend the TUI and hand off stdio
/// directly" approach.
fn print_themed(theme: &crate::theming::Theme, args: std::fmt::Arguments) -> Result<()> {
    execute!(
        std::io::stdout(),
        SetForegroundColor(to_crossterm_color(theme.text)),
        SetBackgroundColor(to_crossterm_color(theme.bg)),
        Print(args),
        ResetColor,
    )?;
    Ok(())
}


fn to_crossterm_color(color: ratatui::style::Color) -> CtColor {
    match color {
        ratatui::style::Color::Rgb(r, g, b) => CtColor::Rgb { r, g, b },
        _ => CtColor::Reset,
    }
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
pub(super) fn run_command_line(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    let input = app.command_line.trim().to_string();
    app.command_line.clear();
    app.command_line_cursor = 0;
    app.command_line_selection_anchor = None;
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

    run_shell_command_lines(app, terminal, &[input])
}


/// If `input` is a `cd` command, returns its argument (trimmed) — or
/// `None` for the argument-less `"cd"` (a no-op, not "go home"; see
/// the plan doc), and `None` for anything that isn't `cd` at all
/// (including a different command that merely starts with "cd", like
/// `"cdw"` — checked via a word boundary, not a bare prefix).
pub(super) fn parse_cd_target(input: &str) -> Option<&str> {
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


/// Appends `line` -- the actual typed/substituted command text, e.g.
/// `svn cleanup --remove-unversioned --remove-ignored "Project Alpha"`
/// -- to `command` as the argument the active shell profile's own
/// `/C`/`-Command`/`-c` flag receives. **Not** a plain `.arg(line)` on
/// Windows -- reported directly, against a real quoted argument: the
/// shell (`cmd`, or the file/folder name it names) ended up seeing the
/// literal characters `"Project Alpha"`, quotes included, instead of
/// the bare name they were meant to delimit (`svn`'s own error message
/// named the culprit outright: `Error resolving case of
/// '"Project Alpha"'`). Root cause: `Command::arg` on Windows
/// re-escapes its argument for `CommandLineToArgvW`-style parsing
/// (wrapping the whole string in an *extra* pair of quotes since it
/// contains spaces, and backslash-escaping every quote already inside
/// it) -- correct for a child that parses its own argv the normal
/// Windows way, but `cmd.exe` (and `powershell.exe -Command`) instead
/// *re-parses* their `/C`/`-Command` argument as an entire command
/// line of their own, using their own, different quoting rules that
/// don't treat a backslash before a quote as an escape at all. The net
/// effect: the user's own quotes around `Project Alpha` survived,
/// mangled, all the way through to `svn`'s own argv.
/// `CommandExt::raw_arg` appends `line` completely unescaped instead,
/// so `cmd`/`powershell` see exactly the text the user typed (or a
/// menu macro substituted), quoted or not, and apply their own parsing
/// to it themselves -- exactly what happens when the same line is
/// typed directly into a `cmd.exe`/PowerShell window. Plain
/// `.arg(line)` is correct (and `raw_arg` isn't available at all) on
/// Unix: `sh -c` receives `line` as one real `argv` element with no
/// re-escaping in between, no reparsing-child mismatch to correct for.
fn append_command_line(command: &mut std::process::Command, line: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.raw_arg(line);
    }
    #[cfg(not(windows))]
    {
        command.arg(line);
    }
}

/// Suspends the TUI and runs each of `lines` in sequence through the
/// active shell profile, inheriting stdio (so interactive programs
/// still work), pausing once at the end for a keypress before
/// redrawing -- shared by the command line's own `Enter` (a single
/// line, `run_command_line` above) and the user menu's own item
/// execution (`explorer::user_menu`, one or more lines run back to
/// back, matching real Far Manager's own multi-line user-menu items —
/// e.g. `git pull` followed by `git remote update ...`). A spawn
/// failure for one line is printed to the suspended console and
/// doesn't stop the remaining lines from still running, same as a
/// plain sequence of typed commands would behave.
pub fn run_shell_command_lines(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>, lines: &[String]) -> Result<()> {
    if lines.is_empty() {
        return Ok(());
    }

    let profile = app.shell_profiles[app.active_shell].clone();
    let cwd = app.active_panel().path.clone();

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    for line in lines {
        debug!(shell = profile.name, %line, cwd = %cwd.display(), "running shell command");
        print_themed(&app.theme, format_args!("{}> {line}\n", cwd.display()))?;
        let mut command = std::process::Command::new(&profile.program);
        command.args(&profile.args_prefix);
        append_command_line(&mut command, line);
        let status = command.current_dir(&cwd).status();
        match status {
            Ok(status) if !status.success() => {
                debug!(?status, "command exited non-zero");
            }
            Err(err) => print_themed(&app.theme, format_args!("failed to launch '{}': {err}\n", profile.program))?,
            Ok(_) => {}
        }
    }
    print_themed(&app.theme, format_args!("\nPress any key to continue...\n"))?;

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


/// `Ctrl+O` -- real Far Manager's own "show/hide panels" toggle.
/// Blocking and stateless, the same shape as `run_shell_command_lines`
/// itself: no `App` field records "panels are currently hidden"
/// anywhere -- this simply doesn't return control to the main loop's
/// own `terminal.draw()` call until the panels should reappear, the
/// same way `run_shell_command_lines` doesn't return until its own
/// "press any key to continue" pause ends.
///
/// Deliberately *doesn't* touch raw mode at all (unlike
/// `run_shell_command_lines`, which disables it so a real subprocess
/// gets normal line-buffered input) -- there's no subprocess here to
/// hand the terminal to, and staying in raw mode means a stray
/// keypress other than `Ctrl+O` is silently swallowed rather than
/// echoed as literal text onto the very console output the user is
/// trying to look at cleanly.
///
/// No "press any key" pause, no message printed at all, unlike
/// `run_shell_command_lines`'s own post-command pause -- the entire
/// point is to reveal whatever's *already* on the real terminal
/// exactly as it is, not add anything on top of it. Only `Ctrl+O`
/// itself brings the panels back; every other key (and mouse event) is
/// silently ignored while hidden, matching real Far Manager's own
/// behavior for this toggle.
pub(super) fn toggle_panels_hidden(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    loop {
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press && key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL) {
                break;
            }
        }
    }

    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    terminal.clear()?;
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

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

    /// Real regression coverage for the actual reported bug, not just a
    /// compile-time check of which `CommandExt` method gets called --
    /// spawns a genuine `cmd.exe` (always present on Windows) and confirms
    /// a quoted argument in the command text survives `cmd`'s own
    /// reparsing intact, the way it would if typed directly into a real
    /// `cmd.exe` window.
    #[cfg(windows)]
    mod append_command_line_tests {
        use std::fs;

        use super::*;
        use crate::test_support::unique_scratch_dir;

        /// `%~1` is a batch-file parameter modifier that strips one
        /// surrounding pair of quotes from `%1` -- exactly what should
        /// happen to the quoted `Project Alpha` below if `cmd.exe`
        /// tokenized the command text itself the normal way (a quoted
        /// argument, quotes meaningful, not literal). The old, broken
        /// `.arg(line)` version of this reported directly as a real `svn`
        /// failure with the quote characters still embedded in the
        /// argument it received (`Error resolving case of
        /// '"Project Alpha"'`) -- if this test is ever reverted to that
        /// version, `%~1` would come back still carrying stray
        /// quote/backslash characters instead of the bare name.
        ///
        /// Shaped to start with a plain word (`call ...`), matching the
        /// real report (`svn cleanup ... "Project Alpha"`) -- deliberately
        /// *not* `"<script>" "Project Alpha"` starting with a quote
        /// itself: `cmd.exe`'s own `/C` handling has a separate, documented
        /// special case for a tail that starts and ends with a quote
        /// (stripping the outer pair under specific conditions, to let a
        /// quoted *executable path* work at all), which doesn't apply to
        /// -- and would give a false result for -- the actual bug being
        /// tested here.
        #[test]
        fn a_quoted_argument_survives_cmds_own_reparsing_unmangled() {
            let dir = unique_scratch_dir("append-command-line");
            let script = dir.join("echo_arg.bat");
            fs::write(&script, "@echo %~1\r\n").unwrap();

            let mut command = std::process::Command::new("cmd");
            command.arg("/C");
            let line = format!("call \"{}\" \"Project Alpha\"", script.display());
            append_command_line(&mut command, &line);

            let output = command.output().unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert_eq!(stdout.trim(), "Project Alpha", "cmd should have tokenized the quoted argument itself, not received it pre-mangled");
        }
    }
}
