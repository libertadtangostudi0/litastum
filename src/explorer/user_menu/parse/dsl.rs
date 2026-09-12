use super::{MenuItem, MenuItemBody};

/// Parses a real `FarMenu.ini`'s content into its top-level items --
/// used only for one-time porting into `LitastumMenu.toml`
/// (`state::port_far_menu`), never for litastum's own file. Far
/// Manager's user-menu format isn't actually `[section]`/`key=value`
/// INI at all (despite the `.ini` extension) -- it's Far's own small
/// nested-block DSL, confirmed against a real published menu
/// (`pkjq/far-git-menu`'s `FarMenu.ini`):
///
/// ```text
/// G: GIT
/// {
/// s: status
/// git status -s
///
/// c: commit
/// {
/// c: Commit
/// git commit -m "!?Commit title?!"
/// }
/// }
/// ```
///
/// - A header line is `<hotkey>: <title>` (`hotkey` is zero or one
///   alphanumeric character right before the *first* `:` on the line —
///   see `is_header_line` for why that's what tells a header apart from
///   an ordinary command line that happens to contain a colon, like a
///   `git log --pretty=format:"..."`).
/// - If the very next line is exactly `{`, the item is a submenu: every
///   line up to the matching `}` is parsed recursively as its own
///   sequence of items.
/// - Otherwise, every following line up to the next header line, a
///   `}`, or end of file is one command to run, in order.
/// - Blank lines separate items but are otherwise ignored; lines whose
///   first non-blank character is `;` are comments, ignored everywhere.
///
/// Malformed input (a line that's neither a valid header nor inside a
/// body -- e.g. stray text before the first header) is skipped rather
/// than causing a parse error or panic; this is a best-effort reader
/// for a hand-edited text file, not a strict format with round-trip
/// guarantees.
pub fn parse(content: &str) -> Vec<MenuItem> {
    let lines: Vec<&str> = content.lines().collect();
    let mut pos = 0;
    parse_items(&lines, &mut pos)
}


fn parse_items(lines: &[&str], pos: &mut usize) -> Vec<MenuItem> {
    let mut items = Vec::new();

    loop {
        skip_blank_and_comment_lines(lines, pos);
        let Some(&line) = lines.get(*pos) else {
            break;
        };
        if line.trim() == "}" {
            break; // caller consumes the closing brace
        }

        let Some((hotkey, title)) = parse_header(line) else {
            // Not a valid header where one was expected -- skip rather
            // than looping forever or misreading it as part of some
            // other item's command body.
            *pos += 1;
            continue;
        };
        *pos += 1;

        if lines.get(*pos).map(|l| l.trim()) == Some("{") {
            *pos += 1;
            let children = parse_items(lines, pos);
            if lines.get(*pos).map(|l| l.trim()) == Some("}") {
                *pos += 1;
            }
            items.push(MenuItem { hotkey, title, body: MenuItemBody::Submenu(children) });
        } else {
            let mut commands = Vec::new();
            while let Some(&line) = lines.get(*pos) {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed == "}" {
                    break;
                }
                if is_comment_line(trimmed) {
                    *pos += 1;
                    continue;
                }
                if is_header_line(line) {
                    break;
                }
                commands.push(line.to_string());
                *pos += 1;
            }
            items.push(MenuItem { hotkey, title, body: MenuItemBody::Commands(commands) });
        }
    }

    items
}


fn skip_blank_and_comment_lines(lines: &[&str], pos: &mut usize) {
    while let Some(&line) = lines.get(*pos) {
        let trimmed = line.trim();
        if trimmed.is_empty() || is_comment_line(trimmed) {
            *pos += 1;
        } else {
            break;
        }
    }
}

fn is_comment_line(trimmed: &str) -> bool {
    trimmed.starts_with(';')
}

/// Whether `line`'s prefix (everything before the first `:`) is a valid
/// hotkey token -- empty, or exactly one alphanumeric character. This
/// is what tells a real header (`"s: status"`, `": log for author"`)
/// apart from a command line that merely contains a colon somewhere
/// (`git log --pretty=format:"..."`, whose prefix up to the first `:`
/// is `"git log --pretty=format"`, nowhere near a valid hotkey).
fn is_header_line(line: &str) -> bool {
    parse_header(line).is_some()
}

fn parse_header(line: &str) -> Option<(Option<char>, String)> {
    let colon = line.find(':')?;
    let prefix = &line[..colon];
    let hotkey = match prefix.chars().count() {
        0 => None,
        1 => {
            let c = prefix.chars().next().unwrap();
            if !c.is_alphanumeric() {
                return None;
            }
            Some(c)
        }
        _ => return None,
    };
    let rest = &line[colon + 1..];
    let title = rest.strip_prefix(' ').unwrap_or(rest).to_string();
    Some((hotkey, title))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_flat_item_with_a_single_command() {
        let items = parse("s: status\ngit status -s\n");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].hotkey, Some('s'));
        assert_eq!(items[0].title, "status");
        assert_eq!(items[0].body, MenuItemBody::Commands(vec!["git status -s".to_string()]));
    }

    #[test]
    fn parses_an_item_with_no_hotkey() {
        let items = parse(": log for author\ngit log\n");
        assert_eq!(items[0].hotkey, None);
        assert_eq!(items[0].title, "log for author");
    }

    #[test]
    fn parses_multiple_command_lines_for_one_item() {
        let items = parse("u: pull\ngit pull\ngit remote update origin --prune\n");
        assert_eq!(items[0].body, MenuItemBody::Commands(vec!["git pull".to_string(), "git remote update origin --prune".to_string()]));
    }

    #[test]
    fn blank_lines_separate_items_without_affecting_them() {
        let items = parse("s: status\ngit status -s\n\nl: log\ngit log\n");
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].title, "log");
    }

    #[test]
    fn comment_lines_are_ignored_everywhere() {
        let items = parse("; a comment\ns: status\n; another comment\ngit status -s\n");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].body, MenuItemBody::Commands(vec!["git status -s".to_string()]));
    }

    #[test]
    fn parses_a_nested_submenu() {
        let items = parse("c: commit\n{\nc: Commit\ngit commit -m \"!?Commit title?!\"\n}\n");
        assert_eq!(items.len(), 1);
        let MenuItemBody::Submenu(children) = &items[0].body else { panic!("expected a submenu") };
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].title, "Commit");
    }

    /// Regression coverage for the whole reason `is_header_line`
    /// exists: a real command line containing colons (a
    /// `--pretty=format:"..."` git argument) must not be
    /// misdetected as a new item header and split off from the
    /// item it belongs to.
    #[test]
    fn a_command_line_containing_colons_is_not_mistaken_for_a_header() {
        let items = parse("l: log\ngit log --pretty=format:\"[%h]%ad(%ar) %an : %s\" --graph\n");
        assert_eq!(items.len(), 1, "should still be a single item, not split by the embedded colons");
        assert_eq!(items[0].body, MenuItemBody::Commands(vec!["git log --pretty=format:\"[%h]%ad(%ar) %an : %s\" --graph".to_string()]));
    }

    #[test]
    fn parses_the_real_far_git_menu_example_end_to_end() {
        let content = "G: GIT\n{\ns: status\ngit status -s\n\nc: commit\n{\nc: Commit\ngit commit -m \"!?Commit title?!\"\n}\n}\n";
        let items = parse(content);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "GIT");
        let MenuItemBody::Submenu(top_children) = &items[0].body else { panic!("expected a submenu") };
        assert_eq!(top_children.len(), 2);
        assert_eq!(top_children[0].title, "status");
        let MenuItemBody::Submenu(commit_children) = &top_children[1].body else { panic!("expected a nested submenu") };
        assert_eq!(commit_children[0].title, "Commit");
    }

    #[test]
    fn empty_content_parses_to_no_items() {
        assert_eq!(parse(""), Vec::new());
        assert_eq!(parse("\n\n; just a comment\n"), Vec::new());
    }

    #[test]
    fn stray_content_before_any_header_is_skipped_not_fatal() {
        let items = parse("this is not a header\ns: status\ngit status -s\n");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "status");
    }
}
