use std::io::Stdout;

use color_eyre::eyre::Result;
use ratatui::{prelude::CrosstermBackend, Terminal};
use tracing::debug;

use crate::app::App;
use crate::command_line::effect::Effect;
use crate::command_line::history::{record_history, save_history};

mod app_paths;

use super::live_command::run_live;


/// Submits `app.command_line`: `cd` moves the active panel right here (a
/// shell's own `cd` couldn't affect our process); `cls`/`clear` becomes
/// `Effect::ClearScreen` (shelling out wiped even the echoed prompt);
/// anything else becomes `Effect::RunShell` -- run in a pseudoconsole
/// with its output on the user screen (`run_shell_command_lines`).
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
        let cwd = app.active_panel().path.clone();
        let failure = match app.active_panel().change_dir(target) {
            Ok(true) => None,
            Ok(false) => Some(format!("cd: no such directory: {target}")),
            Err(err) => Some(format!("cd: {target}: {err}")),
        };
        // Reported: a cd to a missing directory said nothing. The user
        // screen gets the line and the answer, as a shell would show them;
        // over the panels, a notice too, or it'd go unseen.
        if let Some(message) = failure {
            let theme = app.theme;
            app.user_screen.begin_command(&cwd, &input, &theme);
            app.user_screen.push_message(&message, &theme);
            if !app.panels_hidden {
                app.notice = Some(crate::notice::Notice::error(message));
            }
        }
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


/// `line` as the argument after the shell's `/C`/`-Command`/`-c`. It goes
/// onto the command line unescaped (`LiveCommand::spawn`): `cmd` and
/// PowerShell re-parse that argument as a whole command line, and argv
/// escaping mangled quoted arguments. History:
/// docs/history/command-execution.md.
pub(super) fn shell_argument(line: &str) -> String {
    #[cfg(windows)]
    {
        wrap_leading_quote_for_cmd(line)
    }
    #[cfg(not(windows))]
    {
        line.to_string()
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

/// Runs `lines` in sequence through the active shell profile, each in a
/// pseudoconsole with its output live on the user screen (`run_live`),
/// set apart from the previous command's; then back to the panels --
/// `Ctrl+O` shows the output again. Shared by the command line and
/// multi-line F2 menu items.
///
/// `cd` lines are handled here, as Far does: each line is its own shell
/// process, so a shelled-out `cd` wouldn't carry over. The active panel
/// moves and later lines run there; a `cd` to a missing directory stops
/// the item. History: docs/history/command-execution.md.
pub(in crate::command_line) fn run_shell_command_lines(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>, lines: &[String]) -> Result<()> {
    for line in lines {
        let cwd = app.active_panel().path.clone();
        let theme = app.theme;
        app.user_screen.begin_command(&cwd, line, &theme);
        if let Some(target) = parse_cd_target(line) {
            debug!(target, "command line: cd");
            let failure = match app.active_panel().change_dir(target) {
                Ok(true) => None,
                Ok(false) => Some(format!("cd: no such directory: {target}")),
                Err(err) => Some(format!("cd: {target}: {err}")),
            };
            if let Some(message) = failure {
                app.user_screen.push_message(&format!("{message} -- the remaining commands were not run"), &theme);
                break;
            }
            continue;
        }
        run_live(app, terminal, line)?;
    }
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

    /// Real regression coverage for the reported quoting bugs: spawns a
    /// genuine `cmd.exe` in a pseudoconsole, as commands run now, and
    /// confirms a quoted argument in the command text survives `cmd`'s own
    /// reparsing intact, the way it would typed into a real `cmd.exe`.
    #[cfg(windows)]
    mod shell_argument_tests {
        use std::fs;
        use std::time::{Duration, Instant};

        use super::*;
        use crate::test_support::unique_scratch_dir;
        use crate::user_screen::LiveCommand;

        fn run_to_end(line: &str, cwd: &std::path::Path) -> String {
            let command = LiveCommand::spawn("cmd", vec!["/C".into(), shell_argument(line)], cwd, 120, 24).unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            while !command.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            command.output().iter().map(|line| line.to_string()).collect::<Vec<_>>().join("
")
        }

        /// Regression: `.arg(line)` delivered `"Project Alpha"` to `svn`
        /// with the quotes still in it. `%~1` strips one pair of quotes,
        /// so it prints the bare name only if `cmd` tokenized the line
        /// itself. Starts with a plain word (`call`), so `cmd`'s separate
        /// leading-quote rule doesn't apply.
        #[test]
        fn a_quoted_argument_survives_cmds_own_reparsing_unmangled() {
            let dir = unique_scratch_dir("shell-argument");
            let script = dir.join("echo_arg.bat");
            fs::write(&script, "@echo [%~1]
").unwrap();

            let output = run_to_end(&format!("call \"{}\" \"Project Alpha\"", script.display()), &dir);

            assert!(output.contains("[Project Alpha]"), "cmd should have tokenized the quoted argument itself: {output:?}");
        }

        /// Regression: a quoted program path plus a quoted argument (four
        /// quotes) -- without the extra wrap, `cmd` strips the quotes
        /// around the script path.
        #[test]
        fn a_quoted_program_path_followed_by_a_quoted_argument_survives_too() {
            let dir = unique_scratch_dir("shell-argument leading quote");
            let script = dir.join("echo_arg.bat");
            fs::write(&script, "@echo [%~1]
").unwrap();

            let output = run_to_end(&format!("\"{}\" \"Project Alpha\"", script.display()), &dir);

            assert!(output.contains("[Project Alpha]"), "the script's own path must not be corrupted by cmd's leading-quote strip: {output:?}");
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
