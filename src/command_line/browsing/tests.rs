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
