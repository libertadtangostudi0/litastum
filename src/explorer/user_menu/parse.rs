use std::collections::HashSet;

/// One parsed entry from a `LitastumMenu.ini`/`FarMenu.ini` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    /// The single character before the item's own `:` (`"s: status"` ->
    /// `Some('s')`), shown as a prefix in the list but not currently
    /// wired up as an instant-select shortcut -- matches this
    /// codebase's own existing precedent (`TODO/f9-menu.md`'s "no
    /// keyboard shortcut letters" gap on the F9 menu too); `Up`/`Down`/
    /// `Enter` is enough for now. `F1`..`F24`-style hotkeys (real Far
    /// Manager also allows those) aren't recognized as hotkeys at all
    /// here -- a line like `F5: refresh` doesn't match the single-char
    /// prefix rule below, so it falls through to being read as a
    /// hotkey-less item titled `F5: refresh`'s remainder, a known,
    /// narrow gap rather than full parity with every hotkey Far allows.
    pub hotkey: Option<char>,
    pub title: String,
    pub body: MenuItemBody,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuItemBody {
    /// Every line after the header, in order, until the next header
    /// line, a closing `}`, or end of file -- run in sequence when this
    /// item is selected (`command_line::run_shell_command_lines`).
    Commands(Vec<String>),
    /// A `{ ... }` block right after the header -- its own nested
    /// sequence of items, entered by selecting this one.
    Submenu(Vec<MenuItem>),
}


/// Parses a whole `LitastumMenu.ini`/`FarMenu.ini` file's content into
/// its top-level items. Real Far Manager's user-menu format isn't
/// actually `[section]`/`key=value` INI at all (despite the `.ini`
/// extension) -- it's Far's own small nested-block DSL, confirmed
/// against a real published menu (`pkjq/far-git-menu`'s `FarMenu.ini`):
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


/// Substitutes every `!&` in `command` with `cursor_path`'s displayed
/// form (empty string if there's no entry under the cursor) -- Far
/// Manager's own "the file/directory currently selected" macro.
pub fn substitute_cursor_path(command: &str, cursor_path: Option<&std::path::Path>) -> String {
    let replacement = cursor_path.map(|p| p.display().to_string()).unwrap_or_default();
    command.replace("!&", &replacement)
}


/// One `!?Label?Default!` placeholder -- Far Manager's own interactive
/// prompt macro (confirmed against the same real `far-git-menu` file,
/// e.g. `git commit -m "!?Commit title?!"`, `git checkout
/// !?Branch?Master!"`): asks the user for a value (pre-filled with
/// `default`) before running the command, once per unique `label`, and
/// substitutes the answer everywhere that `label` appears.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub label: String,
    pub default: String,
}

/// Every unique `!?Label?Default!` placeholder across `commands`, in
/// first-appearance order. The same `label` appearing again (even with
/// a different `default`) isn't asked twice -- its first occurrence's
/// `default` is what's kept, matching Far Manager's own "each distinct
/// label prompts exactly once" behavior.
pub fn extract_prompts(commands: &[String]) -> Vec<Prompt> {
    let mut prompts = Vec::new();
    let mut seen = HashSet::new();

    for command in commands {
        let mut rest = command.as_str();
        while let Some((_prefix, label, default, after)) = next_placeholder(rest) {
            if seen.insert(label.to_string()) {
                prompts.push(Prompt { label: label.to_string(), default: default.to_string() });
            }
            rest = after;
        }
    }

    prompts
}

/// Substitutes every `!?Label?Default!` in `command` with the matching
/// answer from `answers` (an empty string if `command` names a `label`
/// that isn't in `answers` at all -- shouldn't happen in practice,
/// since `answers` is always built from `extract_prompts` run against
/// the very same commands, but a missing answer degrading to empty
/// text is safer than panicking on it).
pub fn substitute_prompts(command: &str, answers: &[(String, String)]) -> String {
    let mut result = String::new();
    let mut rest = command;

    while let Some((prefix, label, _default, after)) = next_placeholder(rest) {
        result.push_str(prefix);
        let answer = answers.iter().find(|(l, _)| l == label).map(|(_, v)| v.as_str()).unwrap_or("");
        result.push_str(answer);
        rest = after;
    }
    result.push_str(rest);

    result
}

/// Finds the next `!?Label?Default!` placeholder in `text`, if any --
/// returns `(text_before_it, label, default, remainder_after_it)`.
/// Shared by `extract_prompts` (which only reads `label`/`default`) and
/// `substitute_prompts` (which also needs the untouched text on both
/// sides, to rebuild the command with the answer spliced in).
fn next_placeholder(text: &str) -> Option<(&str, &str, &str, &str)> {
    let start = text.find("!?")?;
    let prefix = &text[..start];
    let after_marker = &text[start + 2..];
    let label_end = after_marker.find('?')?;
    let label = &after_marker[..label_end];
    let after_label = &after_marker[label_end + 1..];
    let default_end = after_label.find('!')?;
    let default = &after_label[..default_end];
    let remainder = &after_label[default_end + 1..];
    Some((prefix, label, default, remainder))
}


#[cfg(test)]
mod tests {
    use super::*;

    mod parse_tests {
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

    mod substitute_cursor_path_tests {
        use super::*;
        use std::path::PathBuf;

        #[test]
        fn replaces_every_occurrence() {
            let path = PathBuf::from("C:/dev/file.txt");
            assert_eq!(substitute_cursor_path("git add !& && git status !&", Some(&path)), "git add C:/dev/file.txt && git status C:/dev/file.txt");
        }

        #[test]
        fn empty_string_when_nothing_is_selected() {
            assert_eq!(substitute_cursor_path("git add !&", None), "git add ");
        }

        #[test]
        fn leaves_a_command_with_no_macro_untouched() {
            assert_eq!(substitute_cursor_path("git status -s", None), "git status -s");
        }
    }

    mod prompt_tests {
        use super::*;

        #[test]
        fn extract_prompts_finds_label_and_default() {
            let commands = vec!["git checkout !?Branch?Master!".to_string()];
            let prompts = extract_prompts(&commands);
            assert_eq!(prompts, vec![Prompt { label: "Branch".to_string(), default: "Master".to_string() }]);
        }

        #[test]
        fn extract_prompts_handles_an_empty_default() {
            let commands = vec!["git commit -m \"!?Commit title?!\"".to_string()];
            let prompts = extract_prompts(&commands);
            assert_eq!(prompts, vec![Prompt { label: "Commit title".to_string(), default: String::new() }]);
        }

        #[test]
        fn extract_prompts_deduplicates_the_same_label_across_commands() {
            let commands = vec!["echo !?Name?!".to_string(), "echo hi !?Name?!".to_string()];
            let prompts = extract_prompts(&commands);
            assert_eq!(prompts.len(), 1, "the same label should only be asked once");
        }

        #[test]
        fn extract_prompts_keeps_first_appearance_order() {
            let commands = vec!["echo !?Second?b! !?First?a!".to_string()];
            let prompts = extract_prompts(&commands);
            assert_eq!(prompts.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(), vec!["Second", "First"]);
        }

        #[test]
        fn no_prompts_in_a_plain_command() {
            assert_eq!(extract_prompts(&["git status -s".to_string()]), Vec::new());
        }

        #[test]
        fn substitute_prompts_fills_in_the_answer() {
            let answers = vec![("Branch".to_string(), "feature/x".to_string())];
            assert_eq!(substitute_prompts("git checkout !?Branch?Master!", &answers), "git checkout feature/x");
        }

        #[test]
        fn substitute_prompts_fills_in_every_occurrence_of_the_same_label() {
            let answers = vec![("Name".to_string(), "world".to_string())];
            assert_eq!(substitute_prompts("echo !?Name?! !?Name?!", &answers), "echo world world");
        }

        #[test]
        fn substitute_prompts_uses_empty_string_for_an_unanswered_label() {
            assert_eq!(substitute_prompts("echo !?Name?!", &[]), "echo ");
        }

        #[test]
        fn substitute_prompts_leaves_a_command_with_no_placeholder_untouched() {
            assert_eq!(substitute_prompts("git status -s", &[]), "git status -s");
        }
    }
}
