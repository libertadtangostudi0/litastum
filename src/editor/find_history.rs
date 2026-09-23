use std::fs;

use tracing::{debug, warn};

use crate::history_dir::history_dir;

/// Longest the in-memory search history is allowed to grow — oldest
/// entries drop off the front once exceeded. Same cap as
/// `command_line::history::MAX_HISTORY`, no particular reason to differ.
const MAX_HISTORY: usize = 50;

/// The file name persisted search history is stored under, inside
/// `history_dir()`'s own directory. A separate file from
/// `command_history.txt`, not a shared one — editor search queries and
/// shell commands are different enough kinds of "history" that mixing
/// them would make both harder to browse.
const HISTORY_FILE: &str = "editor_search_history.txt";

/// Loads persisted search history, one query per line, oldest first —
/// same shape as `command_line::history::load_history`. A missing/
/// unreachable `history_dir()`, a missing file, or any read error just
/// means "no history yet", not a startup failure.
pub fn load_history() -> Vec<String> {
    let Some(dir) = history_dir() else {
        debug!("search history: no history directory available, starting empty");
        return Vec::new();
    };
    match fs::read_to_string(dir.join(HISTORY_FILE)) {
        Ok(contents) => contents.lines().map(str::to_string).collect(),
        Err(err) => {
            debug!(%err, "search history: no persisted history to load");
            Vec::new()
        }
    }
}

/// Best-effort — logged and otherwise ignored on failure, same as
/// `command_line::history::save_history`; a failed save shouldn't block
/// closing the search box.
pub fn save_history(history: &[String]) {
    let Some(dir) = history_dir() else {
        warn!("search history: no history directory available, not persisted");
        return;
    };
    if let Err(err) = fs::create_dir_all(&dir) {
        warn!(%err, "search history: failed to create the history directory");
        return;
    }
    if let Err(err) = fs::write(dir.join(HISTORY_FILE), history.join("\n")) {
        warn!(%err, "search history: failed to persist");
    }
}

/// Appends `query` to `history` — called when the search box closes
/// (`Esc`) with a non-empty query, the closest analogue this feature
/// has to the command line's own "record on Enter" (there's no
/// separate "run" step for a search to wait for). Skips a repeat of the
/// immediately-previous entry and caps total length at `MAX_HISTORY`,
/// same rules as `command_line::history::record_history`.
pub fn record_history(history: &mut Vec<String>, query: &str) {
    if history.last().map(String::as_str) == Some(query) {
        return;
    }
    history.push(query.to_string());
    if history.len() > MAX_HISTORY {
        history.remove(0);
    }
}

/// The most-recently-used history entry that starts with `query`
/// (case-insensitive), if it's strictly longer than `query` — `None`
/// for an empty query, no match, or an exact-length match with nothing
/// left to suggest. Ghost-text ordinary suggestion for the search box,
/// requested directly (an inline suggestion, similar to the command
/// line's own) — a *prefix* match, not `command_line::history::suggest_history`'s own
/// substring-anywhere match, since only a prefix match can be shown as
/// dimmed text appended after what's already typed.
pub fn suggest<'a>(history: &'a [String], query: &str) -> Option<&'a str> {
    if query.is_empty() {
        return None;
    }
    let query_lower = query.to_lowercase();
    history
        .iter()
        .rev()
        .find(|entry| entry.to_lowercase().starts_with(&query_lower) && entry.chars().count() > query.chars().count())
        .map(String::as_str)
}


#[cfg(test)]
mod tests {
    use super::*;

    mod record_history_tests {
        use super::*;

        #[test]
        fn appends_new_queries() {
            let mut history = vec!["foo".to_string()];
            record_history(&mut history, "bar");
            assert_eq!(history, vec!["foo", "bar"]);
        }

        #[test]
        fn skips_an_immediate_repeat() {
            let mut history = vec!["foo".to_string()];
            record_history(&mut history, "foo");
            assert_eq!(history, vec!["foo"], "searching the same thing twice in a row shouldn't duplicate it");
        }

        #[test]
        fn allows_a_repeat_that_is_not_immediately_consecutive() {
            let mut history = vec!["foo".to_string(), "bar".to_string()];
            record_history(&mut history, "foo");
            assert_eq!(history, vec!["foo", "bar", "foo"]);
        }

        #[test]
        fn caps_at_max_history_dropping_the_oldest() {
            let mut history: Vec<String> = (0..MAX_HISTORY).map(|_| "placeholder".to_string()).collect();
            record_history(&mut history, "distinct");
            assert_eq!(history.len(), MAX_HISTORY);
            assert_eq!(history.last().unwrap(), "distinct");
        }
    }

    mod suggest_tests {
        use super::*;

        #[test]
        fn suggests_the_most_recent_prefix_match() {
            let history = vec!["error".to_string(), "errno".to_string()];
            assert_eq!(suggest(&history, "err"), Some("errno"), "most recently used should win");
        }

        #[test]
        fn matches_case_insensitively() {
            let history = vec!["TODO".to_string()];
            assert_eq!(suggest(&history, "tod"), Some("TODO"));
        }

        #[test]
        fn empty_query_suggests_nothing() {
            let history = vec!["error".to_string()];
            assert_eq!(suggest(&history, ""), None);
        }

        #[test]
        fn no_prefix_match_suggests_nothing() {
            let history = vec!["error".to_string()];
            assert_eq!(suggest(&history, "warn"), None);
        }

        #[test]
        fn a_substring_match_that_is_not_a_prefix_is_not_suggested() {
            // Unlike command_line::history::suggest_history's own
            // substring-anywhere match -- ghost text can only ever show
            // the *rest* of an entry appended after what's typed, so a
            // match has to start where the query does.
            let history = vec!["cargo build".to_string()];
            assert_eq!(suggest(&history, "build"), None);
        }

        #[test]
        fn an_exact_length_match_suggests_nothing_left_to_add() {
            let history = vec!["error".to_string()];
            assert_eq!(suggest(&history, "error"), None);
        }
    }
}
