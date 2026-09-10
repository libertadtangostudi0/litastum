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
