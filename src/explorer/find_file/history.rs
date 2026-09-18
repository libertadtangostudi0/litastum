use std::fs;

use tracing::{debug, warn};

/// Longest either in-memory history is allowed to grow — oldest entries
/// drop off the front once exceeded. Same cap as
/// `command_line::history::MAX_HISTORY`/`editor::find_history::MAX_HISTORY`,
/// no particular reason to differ.
const MAX_HISTORY: usize = 50;

/// Where the "File name to find" field's own persisted history lives —
/// a plain file in the current working directory, same deliberately-
/// simple-for-now choice `command_line::history::HISTORY_FILE`/
/// `editor::find_history::HISTORY_FILE` already made. A separate file
/// from the content-query history below -- requested directly, asking
/// for the two histories to be kept separate: the two fields are
/// different enough
/// kinds of query (a file name/glob mask vs. a substring to find inside
/// a file) that mixing their history would make recalling either one
/// harder, the same reasoning that already kept editor search history
/// and command history in two separate files instead of one shared one.
pub const NAME_HISTORY_FILE: &str = "find_file_name_history.txt";

/// Where the "Text to find" field's own persisted history lives — see
/// `NAME_HISTORY_FILE`'s own doc comment for why this is a distinct
/// file rather than a shared one.
pub const CONTENT_HISTORY_FILE: &str = "find_file_content_history.txt";

/// Loads persisted history from `path`, one query per line, oldest
/// first — same shape as `editor::find_history::load_history`. A
/// missing file or any read error just means "no history yet," not a
/// startup failure. Parameterized by `path` (rather than a single
/// hardcoded filename, the way the two sibling history modules do it)
/// since this one module serves both of Find file's own histories.
pub fn load_history(path: &str) -> Vec<String> {
    match fs::read_to_string(path) {
        Ok(contents) => contents.lines().map(str::to_string).collect(),
        Err(err) => {
            debug!(%err, path, "find file: no persisted history to load");
            Vec::new()
        }
    }
}

/// Best-effort — logged and otherwise ignored on failure, same as the
/// two sibling history modules; a failed save shouldn't block closing
/// the popup.
pub fn save_history(path: &str, history: &[String]) {
    if let Err(err) = fs::write(path, history.join("\n")) {
        warn!(%err, path, "find file: failed to persist history");
    }
}

/// Appends `query` to `history` — called when `Enter` actually runs a
/// search (`input.rs::run_search`), the closest analogue this feature
/// has to the command line's own "record on run" (there's no separate
/// "submit" step to wait for beyond that). A no-op for an empty query
/// (the field that wasn't used in a given search shouldn't gain an
/// empty-string history entry). Skips a repeat of the immediately-
/// previous entry and caps total length at `MAX_HISTORY`, same rules
/// as the two sibling history modules.
pub fn record_history(history: &mut Vec<String>, query: &str) {
    if query.is_empty() {
        return;
    }
    if history.last().map(String::as_str) == Some(query) {
        return;
    }
    history.push(query.to_string());
    if history.len() > MAX_HISTORY {
        history.remove(0);
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_history_appends_new_queries() {
        let mut history = vec!["foo".to_string()];
        record_history(&mut history, "bar");
        assert_eq!(history, vec!["foo", "bar"]);
    }

    #[test]
    fn record_history_skips_an_empty_query() {
        let mut history = vec!["foo".to_string()];
        record_history(&mut history, "");
        assert_eq!(history, vec!["foo"], "the field that wasn't used in a search shouldn't gain an empty entry");
    }

    #[test]
    fn record_history_skips_an_immediate_repeat() {
        let mut history = vec!["foo".to_string()];
        record_history(&mut history, "foo");
        assert_eq!(history, vec!["foo"], "searching the same thing twice in a row shouldn't duplicate it");
    }

    #[test]
    fn record_history_allows_a_repeat_that_is_not_immediately_consecutive() {
        let mut history = vec!["foo".to_string(), "bar".to_string()];
        record_history(&mut history, "foo");
        assert_eq!(history, vec!["foo", "bar", "foo"]);
    }

    #[test]
    fn record_history_caps_at_max_history_dropping_the_oldest() {
        let mut history: Vec<String> = (0..MAX_HISTORY).map(|i| format!("placeholder_{i}")).collect();
        record_history(&mut history, "distinct");
        assert_eq!(history.len(), MAX_HISTORY);
        assert_eq!(history.last().unwrap(), "distinct");
    }
}
