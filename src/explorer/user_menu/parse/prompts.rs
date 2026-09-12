use std::collections::HashSet;

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

/// Finds the next placeholder in `text`, if any -- either Far's own
/// `!?Label?Default!` or litastum's `{{prompt:Label}}`/
/// `{{prompt:Label:Default}}` (`super::substitution::consume_litastum_token`'s
/// own doc comment explains why litastum has its own syntax at all).
/// Returns `(text_before_it, label, default, remainder_after_it)`.
/// Shared by `extract_prompts` (which only reads `label`/`default`) and
/// `substitute_prompts` (which also needs the untouched text on both
/// sides, to rebuild the command with the answer spliced in) -- both
/// work unmodified against whichever syntax was actually used, since
/// this is the only place that has to know both exist.
fn next_placeholder(text: &str) -> Option<(&str, &str, &str, &str)> {
    let far = text.find("!?").and_then(|start| parse_far_placeholder(text, start));
    let native = text.find("{{prompt:").and_then(|start| parse_native_placeholder(text, start));

    match (far, native) {
        (Some(f), Some(n)) => {
            // Whichever one actually starts earlier in `text` wins --
            // `prefix.len()` *is* that starting offset, since both
            // prefixes are slices measured from the very start of
            // `text`.
            if f.0.len() <= n.0.len() { Some(f) } else { Some(n) }
        }
        (Some(f), None) => Some(f),
        (None, Some(n)) => Some(n),
        (None, None) => None,
    }
}

/// Parses a `!?Label?Default!` placeholder assumed to start at byte
/// offset `start` in `text` (`text[start..]` begins with `!?`) -- `None`
/// if what follows isn't actually a well-formed placeholder (e.g. the
/// degenerate `!?!`, Far's own file-description macro, which
/// `super::substitution::substitute_macros` leaves untouched for the
/// same reason). `pub(super)` since `substitution::consume_far_token`
/// also needs this exact parse (to copy a real placeholder's span out
/// atomically rather than misreading its own closing `!` as an
/// unrelated macro -- see that function's own doc comment for the
/// regression this fixed).
pub(super) fn parse_far_placeholder(text: &str, start: usize) -> Option<(&str, &str, &str, &str)> {
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

/// Parses a `{{prompt:Label}}`/`{{prompt:Label:Default}}` placeholder
/// assumed to start at byte offset `start` in `text`
/// (`text[start..]` begins with `{{prompt:`) -- `None` if there's no
/// matching `}}` at all. A label containing `:` would be ambiguous
/// against its own optional default and isn't attempted here, same
/// narrow scope Far's own `!?Label?Default!` already has (a label
/// containing `?` isn't representable there either).
fn parse_native_placeholder(text: &str, start: usize) -> Option<(&str, &str, &str, &str)> {
    let prefix = &text[..start];
    let after_marker = &text[start + "{{prompt:".len()..];
    let close = after_marker.find("}}")?;
    let inside = &after_marker[..close];
    let remainder = &after_marker[close + 2..];
    let (label, default) = inside.split_once(':').unwrap_or((inside, ""));
    Some((prefix, label, default, remainder))
}


#[cfg(test)]
mod tests {
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

    /// The actual point of the `{{prompt:...}}` syntax: it goes
    /// through the exact same `extract_prompts`/`substitute_prompts`
    /// path as Far's own `!?...?!`, just spelled differently.
    #[test]
    fn extract_prompts_recognizes_the_litastum_style_syntax_with_a_default() {
        let commands = vec!["git checkout {{prompt:Branch:Master}}".to_string()];
        let prompts = extract_prompts(&commands);
        assert_eq!(prompts, vec![Prompt { label: "Branch".to_string(), default: "Master".to_string() }]);
    }

    #[test]
    fn extract_prompts_recognizes_the_litastum_style_syntax_with_no_default() {
        let commands = vec!["echo {{prompt:Name}}".to_string()];
        let prompts = extract_prompts(&commands);
        assert_eq!(prompts, vec![Prompt { label: "Name".to_string(), default: String::new() }]);
    }

    #[test]
    fn substitute_prompts_fills_in_the_litastum_style_placeholder() {
        let answers = vec![("Branch".to_string(), "feature/x".to_string())];
        assert_eq!(substitute_prompts("git checkout {{prompt:Branch:Master}}", &answers), "git checkout feature/x");
    }

    /// Both syntaxes can appear in the same command (e.g. a
    /// hand-edited item mixing a freshly-typed `{{prompt:...}}`
    /// with an old, still-present `!?...?!` from a ported
    /// `FarMenu.ini`) -- whichever one starts earlier in the text
    /// is found first, and both eventually get extracted.
    #[test]
    fn extract_prompts_finds_both_syntaxes_in_the_same_command() {
        let commands = vec!["echo !?First?a! {{prompt:Second:b}}".to_string()];
        let prompts = extract_prompts(&commands);
        assert_eq!(prompts, vec![Prompt { label: "First".to_string(), default: "a".to_string() }, Prompt { label: "Second".to_string(), default: "b".to_string() }]);
    }
}
