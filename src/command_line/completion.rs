use std::fs;
use std::path::Path;

/// A live `Tab`-cycling session, remembered on `App` across repeated
/// `Tab` presses (`App::command_line_completion`) — Windows `cmd.exe`
/// convention: the first `Tab` on a word shows the first match, each
/// further `Tab` steps to the next one (wrapping back to the first
/// after the last), rather than completing to the matches' shared
/// prefix and stopping. Any other edit to the command line
/// (`insert_char`/`backspace`/`Esc`/running it) ends the session — see
/// `browsing::handle_browsing_key`.
pub struct CompletionCycle {
    /// Where the word being completed starts in the line — constant
    /// for the life of one cycle, since every step replaces
    /// `line[word_start..]` wholesale rather than editing in place.
    word_start: usize,
    /// The directory portion of the word as typed (e.g. `"sub\"` in
    /// `"cd sub\tar"`), re-prepended in front of each match in turn.
    dir_part: String,
    /// Every entry in `dir_part` whose name matched the typed prefix
    /// when the cycle started, name plus whether it's a directory
    /// (decides the trailing separator vs. space — see `apply`).
    matches: Vec<(String, bool)>,
    index: usize,
}

/// `Tab`: continues `cycle` if one's already running (steps to the
/// next match), otherwise starts a new one from the word currently
/// under the cursor — the last whitespace-separated "word" in `line`,
/// resolved as a filesystem path relative to `cwd` (an already-absolute
/// word, like `C:\Users\` or `/etc/`, completes from its own root
/// instead — `Path::join`'s own behavior, same as `Panel::change_dir`'s
/// `cd` handling relies on). No matches leaves `line` and `cycle`
/// untouched entirely — no bell, no error, nothing suggested.
///
/// Matching is case-insensitive (Windows filesystems don't
/// distinguish; harmless extra leniency on case-sensitive ones too).
pub fn complete(line: &mut String, cwd: &Path, cycle: &mut Option<CompletionCycle>) {
    if let Some(state) = cycle {
        state.index = (state.index + 1) % state.matches.len();
        apply(line, state);
        return;
    }

    let word_start = line.rfind(char::is_whitespace).map_or(0, |i| i + 1);
    let word = &line[word_start..];
    if word.is_empty() {
        return;
    }

    let dir_end = word.rfind(['/', '\\']).map_or(0, |i| i + 1);
    let (dir_part, prefix) = word.split_at(dir_end);
    let search_dir = cwd.join(dir_part);

    let Ok(entries) = fs::read_dir(&search_dir) else {
        return;
    };

    let mut matches: Vec<(String, bool)> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.to_lowercase()
                .starts_with(&prefix.to_lowercase())
                .then(|| (name, entry.file_type().is_ok_and(|t| t.is_dir())))
        })
        .collect();
    if matches.is_empty() {
        return;
    }
    matches.sort();

    let state = CompletionCycle { word_start, dir_part: dir_part.to_string(), matches, index: 0 };
    apply(line, &state);
    *cycle = Some(state);
}

/// Replaces `line`'s current word with `state`'s currently-selected
/// match — a trailing path separator for a directory (so the next
/// `Tab` press, or typed character, continues *inside* it) or a
/// trailing space for a file (ready for the next argument), matching a
/// normal shell's own completion habit.
fn apply(line: &mut String, state: &CompletionCycle) {
    let (name, is_dir) = &state.matches[state.index];
    let trailer = if *is_dir { std::path::MAIN_SEPARATOR } else { ' ' };
    line.truncate(state.word_start);
    line.push_str(&state.dir_part);
    line.push_str(name);
    line.push(trailer);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::unique_scratch_dir;

    fn scratch_dir() -> std::path::PathBuf {
        unique_scratch_dir("command-line-completion")
    }

    #[test]
    fn complete_single_directory_match_appends_a_trailing_separator() {
        let dir = scratch_dir();
        fs::create_dir_all(dir.join("source")).unwrap();
        let mut line = "cd sou".to_string();
        let mut cycle = None;

        complete(&mut line, &dir, &mut cycle);

        assert_eq!(line, format!("cd source{}", std::path::MAIN_SEPARATOR));
    }

    #[test]
    fn complete_single_file_match_appends_a_trailing_space() {
        let dir = scratch_dir();
        fs::write(dir.join("readme.txt"), b"hi").unwrap();
        let mut line = "cat read".to_string();
        let mut cycle = None;

        complete(&mut line, &dir, &mut cycle);

        assert_eq!(line, "cat readme.txt ");
    }

    #[test]
    fn complete_no_matches_leaves_the_line_untouched() {
        let dir = scratch_dir();
        let mut line = "cat nope".to_string();
        let mut cycle = None;

        complete(&mut line, &dir, &mut cycle);

        assert_eq!(line, "cat nope");
        assert!(cycle.is_none(), "nothing to cycle through");
    }

    #[test]
    fn complete_only_touches_the_last_word() {
        let dir = scratch_dir();
        fs::create_dir_all(dir.join("source")).unwrap();
        let mut line = "cp already-typed sou".to_string();
        let mut cycle = None;

        complete(&mut line, &dir, &mut cycle);

        assert_eq!(line, format!("cp already-typed source{}", std::path::MAIN_SEPARATOR));
    }

    #[test]
    fn complete_matches_case_insensitively() {
        let dir = scratch_dir();
        fs::create_dir_all(dir.join("Source")).unwrap();
        let mut line = "cd sou".to_string();
        let mut cycle = None;

        complete(&mut line, &dir, &mut cycle);

        assert_eq!(line, format!("cd Source{}", std::path::MAIN_SEPARATOR), "should find it despite the case mismatch");
    }

    #[test]
    fn complete_descends_into_an_explicitly_typed_subdirectory() {
        let dir = scratch_dir();
        fs::create_dir_all(dir.join("sub").join("target")).unwrap();
        let separator = std::path::MAIN_SEPARATOR;
        let mut line = format!("cd sub{separator}tar");
        let mut cycle = None;

        complete(&mut line, &dir, &mut cycle);

        assert_eq!(line, format!("cd sub{separator}target{separator}"));
    }

    #[test]
    fn complete_on_an_empty_word_is_a_noop() {
        let dir = scratch_dir();
        let mut line = "cd ".to_string();
        let mut cycle = None;

        complete(&mut line, &dir, &mut cycle);

        assert_eq!(line, "cd ");
    }

    /// The actual point of this session: with several matches, repeated
    /// `Tab` presses (repeated `complete` calls sharing the same
    /// `cycle`) step through *every* match in turn — not just complete
    /// to their shared prefix once and stop, cmd.exe's own convention
    /// for the key.
    #[test]
    fn complete_cycles_through_every_match_on_repeated_tab() {
        let dir = scratch_dir();
        fs::write(dir.join("Cargo.lock"), b"").unwrap();
        fs::write(dir.join("Cargo.toml"), b"").unwrap();
        let mut line = ".\\Cargo.".to_string();
        let mut cycle = None;

        complete(&mut line, &dir, &mut cycle);
        assert_eq!(line, ".\\Cargo.lock ", "first Tab: first match alphabetically");

        complete(&mut line, &dir, &mut cycle);
        assert_eq!(line, ".\\Cargo.toml ", "second Tab: the other match");

        complete(&mut line, &dir, &mut cycle);
        assert_eq!(line, ".\\Cargo.lock ", "third Tab: wraps back around to the first");
    }

    /// A completion cycle survives further `Tab` presses but nothing
    /// else — `handle_browsing_key` is what actually enforces "any
    /// other edit ends it" (it owns `App::command_line_completion`),
    /// this only pins down that `complete` itself doesn't reset
    /// `cycle` on repeated calls unless told to via a fresh `None`.
    #[test]
    fn a_fresh_none_cycle_starts_over_instead_of_continuing() {
        let dir = scratch_dir();
        fs::write(dir.join("Cargo.lock"), b"").unwrap();
        fs::write(dir.join("Cargo.toml"), b"").unwrap();
        let mut line = "Cargo.".to_string();
        let mut cycle = None;
        complete(&mut line, &dir, &mut cycle);
        assert_eq!(line, "Cargo.lock ");

        line = "Cargo.".to_string();
        let mut fresh_cycle = None;
        complete(&mut line, &dir, &mut fresh_cycle);

        assert_eq!(line, "Cargo.lock ", "starts back at the first match, not continuing the old cycle");
    }
}
