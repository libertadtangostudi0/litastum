use std::path::PathBuf;
use std::time::Duration;

use super::background::PendingSearch;

/// Which part of the popup is showing — the typed query, a search
/// actually running in the background, or the results it produced.
/// `Searching` was added for a direct request to compare against real
/// Far Manager's own Find file dialog, which shows live progress and
/// lets `Esc` cancel mid-search rather than blocking with nothing to
/// look at until it's done -- see `background.rs`'s own doc comment for
/// the threading side of this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindFilePhase {
    Typing,
    Searching,
    Results,
}


/// Which of the two `Typing`-phase fields `Tab`/typing currently
/// targets -- Far Manager's own Find file dialog has both a "File
/// names to find" mask and a separate "Text to find" (search-inside-
/// files) field at once, so this needs its own two-field focus rather
/// than the single `query`/`cursor` pair every other text-entry popup
/// in this app gets away with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindFileField {
    Name,
    Content,
}


pub struct FindFileState {
    pub phase: FindFilePhase,
    pub query: String,
    /// Character index into `query` — see `text_field.rs`.
    pub cursor: usize,
    /// `query`'s own active text selection, if any — see
    /// `text_field.rs`'s own selection functions and
    /// `PendingTransfer::selection_anchor`'s doc comment for the
    /// convention this follows (`None` = no selection, `Some(anchor)` =
    /// selecting between `anchor` and `cursor`).
    pub selection_anchor: Option<usize>,
    /// Far Manager's own "Text to find" — an optional substring
    /// (case-insensitive, plain substring only, no glob/regex) a
    /// matching file's own *content* must also contain, on top of
    /// `query`'s name/mask match. Empty means "don't filter by
    /// content" -- same "optional, AND-combined with the other field"
    /// shape real Far's dialog uses. Never applies to directory
    /// results -- a directory has no content to search inside, so one
    /// only ever shows up when this is empty.
    pub content_query: String,
    /// Character index into `content_query` — see `text_field.rs`.
    pub content_cursor: usize,
    /// `content_query`'s own active text selection -- same shape as
    /// `selection_anchor` above, independent of it (each field owns its
    /// own selection, same as each field already owns its own cursor
    /// and history).
    pub content_selection_anchor: Option<usize>,
    /// Which of `query`/`content_query` `Tab` and typed characters
    /// currently reach.
    pub active_field: FindFileField,
    /// Which entry of `query`'s own history (`App::find_file_name_history`)
    /// `Up`/`Down` is currently browsing -- `None` means not currently
    /// browsing (fresh typing, or history browsing was left by an edit).
    /// See `name_history_up`/`_down` below; mirrors
    /// `Editor::search_history_index`'s own shell-`Up`-arrow shape,
    /// just kept per-field here since `FindFileState` already has two
    /// independent fields to browse rather than the editor's one.
    pub name_history_index: Option<usize>,
    /// Same as `name_history_index`, for `content_query`'s own history
    /// (`App::find_file_content_history`).
    pub content_history_index: Option<usize>,
    pub results: Vec<PathBuf>,
    pub selected: usize,
    /// The search currently running on a background thread --
    /// `Some` only during `FindFilePhase::Searching`, `None` in every
    /// other phase (set by `run_search`, cleared by
    /// `background::poll_pending_find_file_search` once it finishes, or
    /// dropped -- after being told to cancel first -- when `Esc` closes
    /// the popup mid-search).
    pub pending: Option<PendingSearch>,
    /// How long the search that produced `results` actually took, wall-
    /// clock -- `PendingSearch::started.elapsed()`, read the moment
    /// `background::poll_pending_find_file_search` notices the
    /// background thread finished, not measured on the background
    /// thread itself. Requested directly so the results popup shows how
    /// long a search actually ran, not just how many results came back.
    /// `None` before any search has run yet (`Typing`/`Searching` never
    /// read this).
    pub search_duration: Option<Duration>,
    /// Whether `results` stopped short of every real match because a
    /// `find_file_max_results`/`find_file_max_visited` cap
    /// (`theming::config::limits()`) was hit, not because the search
    /// genuinely ran out of matches -- `background::poll_pending_find_file_search`
    /// reads this once from `PendingSearch::progress.capped` the moment
    /// a search finishes. Added directly after a side-by-side report
    /// against real Far Manager: a search that happened to hit exactly
    /// `find_file_max_results` (200, the default) looked like a
    /// complete, successful search with no way to tell it wasn't --
    /// `ui/find_file.rs` shows a "+" on the result count instead of a
    /// misleadingly precise total when this is `true`. Never set for a
    /// plain `Esc` cancel -- that's a deliberate stop, not a surprising
    /// incompleteness worth calling out separately.
    pub results_capped: bool,
    /// Feedback from the last `Ctrl+S` export, shown under the results
    /// list until the next export attempt (success or failure — both
    /// get a message, there's no other status-bar surface to put this
    /// on yet). `(label, detail)` — `("Exported to:", "<path>")` or
    /// `("Export failed:", "<error>")` — rendered on two separate
    /// lines rather than one, since a real Downloads path is easily
    /// wide enough to blow past the popup's width on one line.
    pub export_message: Option<(String, String)>,
}


impl FindFileState {
    pub fn new() -> Self {
        Self {
            phase: FindFilePhase::Typing,
            query: String::new(),
            cursor: 0,
            selection_anchor: None,
            content_query: String::new(),
            content_cursor: 0,
            content_selection_anchor: None,
            active_field: FindFileField::Name,
            name_history_index: None,
            content_history_index: None,
            results: Vec::new(),
            selected: 0,
            pending: None,
            search_duration: None,
            results_capped: false,
            export_message: None,
        }
    }

    /// `Up` while the name field is active -- recalls the *previous*
    /// entry in `history` (a shell's own `Up`-arrow convention: first
    /// press shows the most recent past query, each further press steps
    /// one entry further back). A no-op with nothing to recall (`history`
    /// empty, or already at the oldest entry). Mirrors
    /// `Editor::search_history_up` exactly, just against `query`/`cursor`/
    /// `name_history_index` instead of the editor's own search box.
    pub fn name_history_up(&mut self, history: &[String]) {
        Self::history_up(history, &mut self.name_history_index, &mut self.query, &mut self.cursor, &mut self.selection_anchor);
    }

    /// `Down` -- the other half of `name_history_up`: steps back toward
    /// the most recent entry, and past it clears the field entirely. A
    /// no-op while not currently browsing history at all.
    pub fn name_history_down(&mut self, history: &[String]) {
        Self::history_down(history, &mut self.name_history_index, &mut self.query, &mut self.cursor, &mut self.selection_anchor);
    }

    /// Same as `name_history_up`, for `content_query`/`content_cursor`/
    /// `content_history_index`.
    pub fn content_history_up(&mut self, history: &[String]) {
        Self::history_up(history, &mut self.content_history_index, &mut self.content_query, &mut self.content_cursor, &mut self.content_selection_anchor);
    }

    /// Same as `name_history_down`, for `content_query`/`content_cursor`/
    /// `content_history_index`.
    pub fn content_history_down(&mut self, history: &[String]) {
        Self::history_down(history, &mut self.content_history_index, &mut self.content_query, &mut self.content_cursor, &mut self.content_selection_anchor);
    }

    /// Shared mechanics for `name_history_up`/`content_history_up` --
    /// also clears `selection_anchor` (added alongside real cursor/
    /// selection editing for this field): a stale selection from before
    /// recalling a history entry could otherwise point past the end of
    /// the newly recalled (possibly shorter) text.
    fn history_up(history: &[String], index: &mut Option<usize>, field: &mut String, cursor: &mut usize, selection_anchor: &mut Option<usize>) {
        *selection_anchor = None;
        if history.is_empty() {
            return;
        }
        let next_index = match *index {
            None => history.len() - 1,
            Some(current) => current.saturating_sub(1),
        };
        *index = Some(next_index);
        *field = history[next_index].clone();
        *cursor = field.chars().count();
    }

    /// Shared mechanics for `name_history_down`/`content_history_down`
    /// -- see `history_up`'s own doc comment for why this also clears
    /// `selection_anchor`.
    fn history_down(history: &[String], index: &mut Option<usize>, field: &mut String, cursor: &mut usize, selection_anchor: &mut Option<usize>) {
        *selection_anchor = None;
        let Some(current) = *index else {
            return;
        };
        if current + 1 < history.len() {
            *index = Some(current + 1);
            *field = history[current + 1].clone();
        } else {
            *index = None;
            field.clear();
        }
        *cursor = field.chars().count();
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_history_up_recalls_the_most_recent_entry_first() {
        let mut state = FindFileState::new();
        let history = vec!["old.txt".to_string(), "recent.txt".to_string()];

        state.name_history_up(&history);

        assert_eq!(state.query, "recent.txt");
        assert_eq!(state.cursor, "recent.txt".chars().count());
    }

    #[test]
    fn name_history_up_steps_further_back_on_repeated_presses() {
        let mut state = FindFileState::new();
        let history = vec!["old.txt".to_string(), "recent.txt".to_string()];

        state.name_history_up(&history);
        state.name_history_up(&history);

        assert_eq!(state.query, "old.txt");
    }

    #[test]
    fn name_history_up_stops_at_the_oldest_entry() {
        let mut state = FindFileState::new();
        let history = vec!["only.txt".to_string()];

        state.name_history_up(&history);
        state.name_history_up(&history);

        assert_eq!(state.query, "only.txt");
    }

    #[test]
    fn name_history_up_is_a_noop_with_empty_history() {
        let mut state = FindFileState::new();
        state.query = "untouched".to_string();

        state.name_history_up(&[]);

        assert_eq!(state.query, "untouched");
    }

    #[test]
    fn name_history_down_steps_back_toward_the_most_recent_entry() {
        let mut state = FindFileState::new();
        let history = vec!["old.txt".to_string(), "recent.txt".to_string()];
        state.name_history_up(&history);
        state.name_history_up(&history); // now on "old.txt"

        state.name_history_down(&history);

        assert_eq!(state.query, "recent.txt");
    }

    #[test]
    fn name_history_down_past_the_newest_entry_clears_the_field() {
        let mut state = FindFileState::new();
        let history = vec!["recent.txt".to_string()];
        state.name_history_up(&history);

        state.name_history_down(&history);

        assert_eq!(state.query, "");
        assert_eq!(state.name_history_index, None);
    }

    #[test]
    fn name_history_down_is_a_noop_when_not_currently_browsing() {
        let mut state = FindFileState::new();
        state.query = "still typing".to_string();

        state.name_history_down(&["recalled.txt".to_string()]);

        assert_eq!(state.query, "still typing");
    }

    /// `content_history_up`/`_down` are the same mechanics as
    /// `name_history_up`/`_down`, just against the other field --
    /// confirms they're wired to `content_query`/`content_cursor`/
    /// `content_history_index`, not accidentally sharing the name
    /// field's own state.
    #[test]
    fn content_history_up_and_down_operate_on_the_content_field_independently() {
        let mut state = FindFileState::new();
        state.query = "name field untouched".to_string();
        let history = vec!["needle".to_string()];

        state.content_history_up(&history);

        assert_eq!(state.content_query, "needle");
        assert_eq!(state.query, "name field untouched", "browsing the content field's history shouldn't touch the name field");

        state.content_history_down(&history);

        assert_eq!(state.content_query, "");
    }
}
