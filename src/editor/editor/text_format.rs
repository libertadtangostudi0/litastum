//! How a file indents, told when it's opened: what Tab and Shift+Tab do
//! (`indent.rs`). Reported: after a save, VS Code showed the indentation
//! broken -- Tab typed `\t` into a file indented with spaces. Its line
//! breaks: `line_endings.rs`. History: docs/history/editor-saving.md.

use std::path::Path;

use edtui::{Lines, RowIndex};

/// How many rows are looked at to tell the indentation: enough to see
/// the file's habit, without a huge file paying for every row.
const ROWS_SAMPLED: usize = 10_000;

/// The indent width when the file shows none, and a tab's width when
/// counting columns in a file indented with tabs.
const DEFAULT_WIDTH: usize = 4;

/// Files indented with tabs while they have no indented line yet: a
/// Makefile's recipe lines need a tab, and `gofmt` writes tabs.
const TAB_FILE_NAMES: &[&str] = &["makefile", "gnumakefile"];
const TAB_EXTENSIONS: &[&str] = &["mk", "mak", "go"];


/// How a file indents: what Tab types and what Shift+Tab takes away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Indent {
    Tabs,
    /// Spaces, this many a level.
    Spaces(usize),
}

impl Indent {
    /// Told from the file's indented lines: tabs if more of them start
    /// with a tab, else spaces, as wide as the most common step between
    /// one line's indent and the next one's. A file with no indented line
    /// gets `DEFAULT_WIDTH` spaces, or tabs where its kind needs them.
    pub(super) fn of(lines: &Lines, path: &Path) -> Self {
        let mut tab_rows = 0;
        let mut space_rows = 0;
        // How often each step between consecutive indents, 2..=8, occurs.
        let mut steps = [0usize; 9];
        let mut previous = 0;
        for row in 0..lines.len().min(ROWS_SAMPLED) {
            let Some(chars) = lines.get(RowIndex::new(row)) else {
                continue;
            };
            if chars.iter().all(|c| c.is_whitespace()) {
                continue;
            }
            let spaces = chars.iter().take_while(|&&c| c == ' ').count();
            if chars[spaces] == '\t' {
                if spaces == 0 {
                    tab_rows += 1;
                }
                continue;
            }
            // One space is a comment's continuation (` * text`), not a
            // level.
            if spaces >= 2 {
                space_rows += 1;
            }
            let step = spaces.abs_diff(previous);
            if (2..steps.len()).contains(&step) {
                steps[step] += 1;
            }
            previous = spaces;
        }
        if tab_rows > space_rows {
            return Self::Tabs;
        }
        if space_rows > 0 {
            // The smallest of equally common steps.
            let width = (2..steps.len()).filter(|&step| steps[step] > 0).max_by_key(|&step| (steps[step], std::cmp::Reverse(step)));
            return Self::Spaces(width.unwrap_or(DEFAULT_WIDTH));
        }
        if needs_tabs(path) {
            Self::Tabs
        } else {
            Self::Spaces(DEFAULT_WIDTH)
        }
    }

    pub(super) fn width(self) -> usize {
        match self {
            Self::Tabs => DEFAULT_WIDTH,
            Self::Spaces(width) => width,
        }
    }

    /// The column after `before`, a tab reaching the next stop.
    pub(super) fn column(self, before: &[char]) -> usize {
        let width = self.width();
        before.iter().fold(0, |column, &c| if c == '\t' { column / width * width + width } else { column + 1 })
    }

    /// What Tab types at `column`: a tab, or spaces to the next stop.
    pub(super) fn tab_text(self, column: usize) -> Vec<char> {
        match self {
            Self::Tabs => vec!['\t'],
            Self::Spaces(width) => vec![' '; width - column % width],
        }
    }

    /// How many characters at `row`'s start Shift+Tab takes: one tab, or
    /// the spaces back to the previous stop.
    pub(super) fn outdent_len(self, row: &[char]) -> usize {
        if row.first() == Some(&'\t') {
            return 1;
        }
        let spaces = row.iter().take_while(|&&c| c == ' ').count();
        let width = self.width();
        // Spaces short of a stop, then a tab: both go, one level.
        if spaces < width && row.get(spaces) == Some(&'\t') {
            return spaces + 1;
        }
        let back = match spaces % width {
            0 => width,
            partial => partial,
        };
        back.min(spaces)
    }
}


fn needs_tabs(path: &Path) -> bool {
    let lower = |name: Option<&std::ffi::OsStr>| name.and_then(|name| name.to_str()).map(str::to_ascii_lowercase);
    lower(path.file_name()).is_some_and(|name| TAB_FILE_NAMES.contains(&name.as_str()))
        || lower(path.extension()).is_some_and(|extension| TAB_EXTENSIONS.contains(&extension.as_str()))
}


#[cfg(test)]
mod tests {
    use super::*;

    fn indent_of(text: &str) -> Indent {
        Indent::of(&Lines::from(text), Path::new("a.rs"))
    }

    #[test]
    fn spaces_are_told_with_their_width() {
        assert_eq!(indent_of("fn a() {\n    b();\n    if c {\n        d();\n    }\n}\n"), Indent::Spaces(4));
        assert_eq!(indent_of("a:\n  b:\n    c: 1\n  d: 2\n"), Indent::Spaces(2));
        assert_eq!(indent_of("<a>\n        <b/>\n</a>\n"), Indent::Spaces(8));
    }

    #[test]
    fn tabs_are_told_by_the_lines_starting_with_one() {
        assert_eq!(indent_of("fn a() {\n\tb();\n\tif c {\n\t\td();\n\t}\n}\n"), Indent::Tabs);
        assert_eq!(indent_of("a\n\tb\n\t    aligned\n"), Indent::Tabs, "spaces after a tab align, they don't indent");
    }

    #[test]
    fn the_more_common_style_wins_a_mixed_file() {
        assert_eq!(indent_of("a\n\tb\n\tc\n    d\n"), Indent::Tabs);
        assert_eq!(indent_of("a\n    b\n    c\n\td\n"), Indent::Spaces(4));
    }

    #[test]
    fn a_comments_one_space_is_not_an_indent() {
        let text = "/**\n * doc\n * more\n */\nfn a() {\n\tb();\n}\n";
        assert_eq!(indent_of(text), Indent::Tabs);
    }

    #[test]
    fn deeper_jumps_dont_hide_the_width() {
        // Two levels at once (8) now and then; most steps are 4.
        let text = "a {\n    b {\n        c\n    }\n    d(\n            e)\n    f\n}\n";
        assert_eq!(indent_of(text), Indent::Spaces(4));
    }

    #[test]
    fn a_file_with_no_indent_gets_four_spaces_or_its_kinds_tabs() {
        assert_eq!(indent_of("a\nb\n"), Indent::Spaces(4));
        assert_eq!(indent_of(""), Indent::Spaces(4));
        assert_eq!(Indent::of(&Lines::from("all:\n"), Path::new("Makefile")), Indent::Tabs);
        assert_eq!(Indent::of(&Lines::from("x"), Path::new("rules.mk")), Indent::Tabs);
        assert_eq!(Indent::of(&Lines::from("package a\n"), Path::new("a.go")), Indent::Tabs);
        assert_eq!(Indent::of(&Lines::from("all:\n    x\n"), Path::new("Makefile")), Indent::Spaces(4), "what's there wins");
    }

    #[test]
    fn tab_types_spaces_to_the_next_stop() {
        let spaces = Indent::Spaces(4);
        assert_eq!(spaces.tab_text(0), vec![' '; 4]);
        assert_eq!(spaces.tab_text(1), vec![' '; 3]);
        assert_eq!(spaces.tab_text(4), vec![' '; 4]);
        assert_eq!(spaces.tab_text(6), vec![' '; 2]);
        assert_eq!(Indent::Spaces(2).tab_text(3), vec![' '; 1]);
        assert_eq!(Indent::Tabs.tab_text(3), vec!['\t']);
    }

    #[test]
    fn a_tab_counts_to_the_next_stop() {
        let chars: Vec<char> = "\tab\tc".chars().collect();
        assert_eq!(Indent::Spaces(4).column(&chars[..1]), 4);
        assert_eq!(Indent::Spaces(4).column(&chars[..3]), 6);
        assert_eq!(Indent::Spaces(4).column(&chars[..4]), 8);
        assert_eq!(Indent::Spaces(2).column(&chars[..1]), 2);
    }

    #[test]
    fn shift_tab_takes_one_level() {
        let row = |text: &str| text.chars().collect::<Vec<_>>();
        let spaces = Indent::Spaces(4);
        assert_eq!(spaces.outdent_len(&row("        a")), 4);
        assert_eq!(spaces.outdent_len(&row("      a")), 2, "back to the previous stop");
        assert_eq!(spaces.outdent_len(&row("  a")), 2);
        assert_eq!(spaces.outdent_len(&row("a")), 0);
        assert_eq!(spaces.outdent_len(&row("")), 0);
        assert_eq!(spaces.outdent_len(&row("\t\ta")), 1, "a tab in a spaces file");
        assert_eq!(spaces.outdent_len(&row("  \ta")), 3, "spaces short of a stop, then a tab");
        assert_eq!(Indent::Tabs.outdent_len(&row("\t\ta")), 1);
        assert_eq!(Indent::Tabs.outdent_len(&row("    a")), 4, "spaces in a tabs file");
    }
}
