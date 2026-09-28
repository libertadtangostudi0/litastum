use std::io::Stdout;

use color_eyre::eyre::Result;
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    style::{Color as CtColor, Print, ResetColor, SetForegroundColor},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::CrosstermBackend, Terminal};
use tracing::debug;

use crate::app::App;
use crate::command_line::effect::Effect;
use crate::command_line::history::{record_history, save_history};

mod app_paths;

use super::hidden_console::is_ctrl_o;


/// Prints one of litastum's own lines (the echoed `"{cwd}> "` prompt) to
/// the suspended console in `theme.text`. Foreground only: the terminal's
/// default background isn't necessarily `theme.bg`. The command's own
/// output is inherited stdio and never recolored.
pub(super) fn print_themed(theme: &crate::theming::Theme, args: std::fmt::Arguments) -> Result<()> {
    execute!(std::io::stdout(), SetForegroundColor(to_crossterm_color(theme.text)), Print(args), ResetColor)?;
    Ok(())
}


/// Discards every input event queued while a child process owned the
/// console (keys pressed meanwhile queue at the OS level; a stray
/// `Enter` would otherwise run as an empty command and reprint the
/// prompt). Call right after `enable_raw_mode()`. Returns `true` if a
/// `Ctrl+O` was among them -- that one carries intent and the hidden
/// console honors it. History: docs/history/command-execution.md.
pub(super) fn drain_stale_input() -> Result<bool> {
    let mut saw_ctrl_o = false;
    while event::poll(std::time::Duration::from_secs(0))? {
        let Event::Key(key) = event::read()? else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        // Same normalization `toggle_panels_hidden`'s own loop applies
        // to a live-read event -- a queued Ctrl+O typed under a
        // non-Latin layout would otherwise never match here either.
        let key = crate::keyboard_layout::normalize_ctrl_shortcut(key);
        if is_ctrl_o(key) {
            saw_ctrl_o = true;
        }
    }
    Ok(saw_ctrl_o)
}


fn to_crossterm_color(color: ratatui::style::Color) -> CtColor {
    match color {
        ratatui::style::Color::Rgb(r, g, b) => CtColor::Rgb { r, g, b },
        _ => CtColor::Reset,
    }
}


/// Submits `app.command_line`: `cd` moves the active panel right here (a
/// shell's own `cd` couldn't affect our process); `cls`/`clear` becomes
/// `Effect::ClearScreen` (shelling out wiped even the echoed prompt);
/// anything else becomes `Effect::RunShell` -- the TUI is suspended and
/// the line runs with inherited stdio, so interactive programs work.
pub(crate) fn submit_command_line(app: &mut App) -> Result<Effect> {
    let input = app.command_line.text().trim().to_string();
    app.command_line.clear();
    if input.is_empty() {
        return Ok(Effect::None);
    }
    record_history(app, &input);
    save_history(&app.command_history);

    if let Some(target) = parse_cd_target(&input) {
        debug!(target, "command line: cd");
        app.active_panel().change_dir(target)?;
        return Ok(Effect::None);
    }
    if input == "cls" || input == "clear" {
        debug!("command line: clear screen (handled directly, no subprocess)");
        return Ok(Effect::ClearScreen);
    }
    Ok(Effect::RunShell(vec![input]))
}


/// If `input` is a `cd` command, its target -- `None` for a bare `cd`
/// (a no-op, not "go home") and for anything else, including `cdw`.
/// Understands `cmd.exe`'s `cd /d <path>` and a quoted path.
pub(super) fn parse_cd_target(input: &str) -> Option<&str> {
    let rest = input.strip_prefix("cd")?;
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None; // e.g. "cdw ..", not "cd"
    }
    let mut target = rest.trim();
    if let Some(after_switch) = target.strip_prefix("/d").or_else(|| target.strip_prefix("/D")) {
        if after_switch.is_empty() || after_switch.starts_with(char::is_whitespace) {
            target = after_switch.trim();
        }
    }
    let target = target.strip_prefix('"').and_then(|unquoted| unquoted.strip_suffix('"')).unwrap_or(target);
    if target.is_empty() {
        None
    } else {
        Some(target)
    }
}


/// Rewrites `line`'s first word to an absolute path when it's a bare
/// name (no `\`, `/`, `:`) that only resolves through App Paths, not
/// `PATH` -- GUI apps like `devenv` register there. Everything else,
/// including a quoted first word (already a path), is left untouched.
pub(super) fn resolve_app_paths_command(line: &str) -> String {
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


/// Appends `line` as the argument of the shell's `/C`/`-Command`/`-c`.
/// On Windows via `raw_arg`, unescaped: `cmd`/PowerShell re-parse that
/// argument as a whole command line, and `Command::arg`'s argv escaping
/// mangled quoted arguments. History: docs/history/command-execution.md.
pub(super) fn append_command_line(command: &mut std::process::Command, line: &str) {
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


/// Wraps `line` in one extra pair of quotes if it starts with `"`: with
/// more than two quotes, `cmd /C` strips the first and last character,
/// which would otherwise be the quotes around an exe path with spaces
/// (as `resolve_app_paths_command` produces). History: docs/history/command-execution.md.
#[cfg(windows)]
fn wrap_leading_quote_for_cmd(line: &str) -> String {
    if line.starts_with('"') {
        format!("\"{line}\"")
    } else {
        line.to_string()
    }
}

/// Suspends the TUI, runs `lines` in sequence through the active shell
/// profile with inherited stdio, and returns straight to the panels (no
/// "press any key" pause -- `Ctrl+O` shows the output again). Shared by
/// the command line and multi-line F2 menu items. A failed spawn is
/// printed and the rest still run.
///
/// `cd` lines are handled here, as Far does: each line is its own shell
/// process, so a shelled-out `cd` wouldn't carry over. The active panel
/// moves and later lines run there; a `cd` to a missing directory stops
/// the item. History: docs/history/command-execution.md.
pub(in crate::command_line) fn run_shell_command_lines(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>, lines: &[String]) -> Result<()> {
    if lines.is_empty() {
        return Ok(());
    }

    let profile = app.shell_profiles[app.active_shell].clone();
    let mut cwd = app.active_panel().path.clone();

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    for line in lines {
        debug!(shell = profile.name, %line, cwd = %cwd.display(), "running shell command");
        print_themed(&app.theme, format_args!("{}> {line}\n", cwd.display()))?;
        if let Some(target) = parse_cd_target(line) {
            if !app.active_panel().change_dir(target)? {
                print_themed(&app.theme, format_args!("cd: no such directory: {target} -- the remaining commands were not run\n"))?;
                break;
            }
            cwd = app.active_panel().path.clone();
            continue;
        }
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
    // A queued Ctrl+O is moot: this returns to the panels anyway.
    let _ = drain_stale_input()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    terminal.clear()?;

    app.active_panel().reload()?;
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

        /// Found while fixing `cd` in user-menu items: `cmd.exe`'s own
        /// `/d` switch and a quoted path used to end up inside the path.
        #[test]
        fn parse_cd_target_understands_cmds_slash_d_and_quotes() {
            assert_eq!(parse_cd_target(r"cd /d W:\WorkCopies\rust"), Some(r"W:\WorkCopies\rust"));
            assert_eq!(parse_cd_target(r"cd /D W:\x"), Some(r"W:\x"));
            assert_eq!(parse_cd_target(r#"cd "W:\Work Copies""#), Some(r"W:\Work Copies"));
            assert_eq!(parse_cd_target(r#"cd /d "W:\Work Copies""#), Some(r"W:\Work Copies"));
            assert_eq!(parse_cd_target("cd /d"), None, "the switch alone names no directory");
            assert_eq!(parse_cd_target("cd /dir"), Some("/dir"), "only a standalone /d is the switch");
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

        /// Regression: `.arg(line)` delivered `"Project Alpha"` to `svn`
        /// with the quotes still in it. `%~1` strips one pair of quotes,
        /// so it prints the bare name only if `cmd` tokenized the line
        /// itself. Starts with a plain word (`call`), so `cmd`'s separate
        /// leading-quote rule doesn't apply.
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

        /// Regression: a quoted program path plus a quoted argument (four
        /// quotes) -- without the extra wrap, `cmd` strips the quotes
        /// around the script path.
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
