use std::io::Stdout;

use color_eyre::eyre::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    style::{Color as CtColor, Print, ResetColor, SetForegroundColor},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::CrosstermBackend, Terminal};
use tracing::debug;

use crate::app::App;
use crate::command_line::history::{record_history, save_history};

mod app_paths;

/// Prints one line to the real (TUI-suspended) console in `theme.text`
/// -- as close as this project's "inherit stdio, don't capture it"
/// design (see `.claude/rules/litastum-command-line.md`) can get to
/// real Far Manager's own `CommandLine.UserScreen` color group. This
/// only colors litastum's *own* printed lines (the echoed `"{cwd}> "`
/// prompt) -- the shelled-out command's own output is never touched,
/// since it's real inherited stdio, not something rendered through our
/// own buffer the way Far's full-screen text-mode architecture lets it
/// recolor everything (including a child process's output).
/// Reproducing that would need a PTY-based capture-and-recolor layer --
/// a much bigger redesign than this project's current "suspend the TUI
/// and hand off stdio directly" approach.
///
/// **Foreground only, no explicit background** -- reported directly
/// from a screenshot: painting `theme.bg` behind these lines made them
/// stand out as a highlighted-looking rectangle, since a real
/// terminal's own default background is whatever the user has it set
/// to, not necessarily `theme.bg` (the same reason
/// `.claude/rules/litastum-popup-design.md`'s own popup-fill saga
/// eventually gave up trying to match an untouched default by painting
/// a guessed color over it). Leaving the background alone lets these
/// lines blend into the same real background every other line on this
/// suspended console already sits on.
fn print_themed(theme: &crate::theming::Theme, args: std::fmt::Arguments) -> Result<()> {
    execute!(std::io::stdout(), SetForegroundColor(to_crossterm_color(theme.text)), Print(args), ResetColor)?;
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
/// Rewrites `line`'s first word to an absolute path if it's a bare
/// executable name (no `\`, `/`, or `:` — i.e. not already a path of
/// some kind) that only resolves through `app_paths::resolve`, not
/// `cmd.exe`'s own `PATH` search. Leaves `line` untouched for the
/// overwhelming majority of typed commands (a real word-in-`PATH` like
/// `svn`/`git`, or nothing registered under that name at all) — this
/// is meant to catch the narrow "GUI app registered via App Paths
/// instead of `PATH`" case (`devenv`, and the same mechanism most other
/// installed IDEs/editors use), not to replace `PATH` resolution.
///
/// Doesn't try to handle a first word that's itself quoted (e.g.
/// `"my program" arg`) — a quoted first word already implies the user
/// typed an actual path (quoting only ever exists to protect spaces in
/// one), not the bare unadorned name this registry key is keyed by, so
/// there's nothing this lookup could usefully add there.
fn resolve_app_paths_command(line: &str) -> String {
    let trimmed = line.trim_start();
    let leading_ws = &line[..line.len() - trimmed.len()];
    let (word, rest) = trimmed.split_at(trimmed.find(char::is_whitespace).unwrap_or(trimmed.len()));

    if word.is_empty() || word.contains(['\\', '/', ':']) {
        return line.to_string();
    }

    match app_paths::resolve(word) {
        Some(resolved) => format!("{leading_ws}\"{}\"{rest}", resolved.display()),
        None => line.to_string(),
    }
}


fn append_command_line(command: &mut std::process::Command, line: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.raw_arg(&wrap_leading_quote_for_cmd(line));
    }
    #[cfg(not(windows))]
    {
        command.arg(line);
    }
}


/// Wraps `line` in one extra outer pair of quotes if it already starts
/// with a `"` -- otherwise returns it unchanged.
///
/// Reported directly against `resolve_app_paths_command`'s own
/// substitution: `svn status "RFI15.0"` became
/// `"C:\Program Files\SlikSvn\bin\svn.exe" status "RFI15.0"` (`svn` is
/// registered under App Paths *as well as* being on `PATH`), and
/// `cmd.exe` came back with `'C:\Program' is not recognized...` --
/// `cmd /?` documents the exact mechanism: `/C`'s own argument only
/// keeps its quotes intact as literally written when it contains
/// *exactly* two quote characters total; otherwise (four, here -- two
/// around the resolved path, two around `"RFI15.0"`) `cmd.exe` falls
/// back to unconditionally stripping just the first and last character
/// of the whole string when the first one happens to be a quote --
/// which are the quotes protecting the exe path's own spaces, not some
/// outer wrapper we ever intended. Adding one more (deliberately
/// redundant) outer quote pair makes that blind strip remove *those*
/// instead, leaving the real, inner quoting -- around the exe path and
/// around `"RFI15.0"` -- completely untouched.
///
/// Never triggers for an ordinary typed command (`svn status ...`,
/// `cd ..`, ...) -- those never start with a quote in the first place,
/// so `cmd.exe`'s own straightforward parsing already handles them
/// (see `append_command_line`'s own doc comment, and its regression
/// test, for the *other* cmd quoting quirk this project already works
/// around).
#[cfg(windows)]
fn wrap_leading_quote_for_cmd(line: &str) -> String {
    if line.starts_with('"') {
        format!("\"{line}\"")
    } else {
        line.to_string()
    }
}

/// Suspends the TUI and runs each of `lines` in sequence through the
/// active shell profile, inheriting stdio (so interactive programs
/// still work), then returns straight to the panels -- shared by the
/// command line's own `Enter` (a single line, `run_command_line` above)
/// and the user menu's own item execution (`explorer::user_menu`, one
/// or more lines run back to back, matching real Far Manager's own
/// multi-line user-menu items — e.g. `git pull` followed by `git remote
/// update ...`). A spawn failure for one line is printed to the
/// suspended console and doesn't stop the remaining lines from still
/// running, same as a plain sequence of typed commands would behave.
///
/// **No "press any key" pause** -- reported directly as an unwanted
/// extra keypress every single time, on top of the command's own
/// `Enter`. An earlier version paused here so fast-scrolling output
/// wouldn't vanish the instant the panels redrew over it; dropped
/// anyway, per that report -- `Ctrl+O` (`toggle_panels_hidden`) already
/// covers "I want to actually look at what a command printed" as its
/// own dedicated, non-transient view, so this pause was only ever
/// protecting against losing output nobody asked to look at again.
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
        append_command_line(&mut command, &resolve_app_paths_command(line));
        let status = command.current_dir(&cwd).status();
        match status {
            Ok(status) if !status.success() => {
                debug!(?status, "command exited non-zero");
            }
            Err(err) => print_themed(&app.theme, format_args!("failed to launch '{}': {err}\n", profile.program))?,
            Ok(_) => {}
        }
    }
    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    terminal.clear()?;

    app.active_panel().reload()?;
    Ok(())
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
pub(super) fn toggle_panels_hidden(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    let mut input = String::new();
    print_prompt(app)?;

    loop {
        let Event::Key(key) = event::read()? else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL) {
            break;
        }

        match key.code {
            KeyCode::Enter => {
                execute!(std::io::stdout(), Print("\n"))?;
                let line = input.trim().to_string();
                input.clear();
                if !line.is_empty() {
                    record_history(app, &line);
                    save_history(&app.command_history);
                    run_single_line_on_console(app, &line)?;
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
fn run_single_line_on_console(app: &mut App, line: &str) -> Result<()> {
    if let Some(target) = parse_cd_target(line) {
        debug!(target, "hidden console: cd");
        app.active_panel().change_dir(target)?;
        return Ok(());
    }

    if line == "cls" || line == "clear" {
        execute!(std::io::stdout(), crossterm::terminal::Clear(crossterm::terminal::ClearType::All), crossterm::cursor::MoveTo(0, 0))?;
        return Ok(());
    }

    let profile = app.shell_profiles[app.active_shell].clone();
    let cwd = app.active_panel().path.clone();
    let mut command = std::process::Command::new(&profile.program);
    command.args(&profile.args_prefix);
    append_command_line(&mut command, &resolve_app_paths_command(line));

    disable_raw_mode()?;
    let status = command.current_dir(&cwd).status();
    enable_raw_mode()?;

    match status {
        Ok(status) if !status.success() => {
            debug!(?status, "command exited non-zero");
        }
        Err(err) => print_themed(&app.theme, format_args!("failed to launch '{}': {err}\n", profile.program))?,
        Ok(_) => {}
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    /// `app_paths::resolve` itself depends on real, per-machine registry
    /// state, so these only cover the deterministic short-circuits --
    /// the cases `resolve_app_paths_command` must never even query the
    /// registry for.
    mod resolve_app_paths_command_tests {
        use super::*;

        #[test]
        fn leaves_an_already_pathlike_first_word_untouched() {
            assert_eq!(resolve_app_paths_command(r"C:\tools\devenv.exe /build"), r"C:\tools\devenv.exe /build");
            assert_eq!(resolve_app_paths_command("./run.sh --flag"), "./run.sh --flag");
            assert_eq!(resolve_app_paths_command("git:status"), "git:status");
        }

        #[test]
        fn leaves_a_bare_name_with_nothing_registered_untouched() {
            assert_eq!(resolve_app_paths_command("definitely-not-a-real-command-xyz123 --version"), "definitely-not-a-real-command-xyz123 --version");
        }

        #[test]
        fn preserves_leading_whitespace_and_the_rest_of_the_line() {
            assert_eq!(resolve_app_paths_command("  definitely-not-a-real-command-xyz123 arg1 arg2"), "  definitely-not-a-real-command-xyz123 arg1 arg2");
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

        /// The actual reported bug: `wrap_leading_quote_for_cmd`'s own
        /// case, previously untested -- a quoted *program path* (what
        /// `resolve_app_paths_command` produces) followed by a quoted
        /// *argument*, so the whole line both starts and ends with a
        /// quote and carries four quote characters total, not two.
        /// Without the extra outer wrap, `cmd.exe`'s own "not exactly
        /// two quotes" fallback blindly strips the first and last
        /// character of the line -- exactly the quotes protecting the
        /// script path's own spaces -- and `%~1` below would come back
        /// having eaten part of the path instead of the real argument.
        #[test]
        fn a_quoted_program_path_followed_by_a_quoted_argument_survives_too() {
            let dir = unique_scratch_dir("append-command-line leading quote");
            let script = dir.join("echo_arg.bat");
            fs::write(&script, "@echo %~1\r\n").unwrap();

            let mut command = std::process::Command::new("cmd");
            command.arg("/C");
            let line = format!("\"{}\" \"Project Alpha\"", script.display());
            append_command_line(&mut command, &line);

            let output = command.output().unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert_eq!(stdout.trim(), "Project Alpha", "the script's own path must not be corrupted by cmd's leading-quote strip");
        }
    }

    #[cfg(windows)]
    mod wrap_leading_quote_for_cmd_tests {
        use super::*;

        #[test]
        fn wraps_a_line_that_already_starts_with_a_quote() {
            let line = r#""C:\Program Files\SlikSvn\bin\svn.exe" status "RFI15.0""#;
            let expected = format!("\"{line}\"");
            assert_eq!(wrap_leading_quote_for_cmd(line), expected);
        }

        #[test]
        fn leaves_an_ordinary_unquoted_command_untouched() {
            assert_eq!(wrap_leading_quote_for_cmd(r#"svn status "RFI15.0""#), r#"svn status "RFI15.0""#);
        }
    }
}
