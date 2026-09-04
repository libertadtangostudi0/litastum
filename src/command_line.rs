/// Pure logic for the always-live command line at the bottom of the
/// browser (Far Manager-style — see `.claude/rules/litastum-stack.md`
/// for why it's append/backspace-only, no cursor movement). Kept
/// separate from `App`/`main.rs` so it's testable without a terminal.


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
