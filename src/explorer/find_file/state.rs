use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use crate::text_field::TextField;

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
    /// "File name to find" -- a name or glob mask.
    pub query: TextField,
    /// Far's "Text to find": an optional plain, case-insensitive substring a
    /// file's content must also contain. Empty means no content filter;
    /// directories only match while it's empty.
    pub content_query: TextField,
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
    /// Marked results, by index -- for picking two results from unrelated
    /// directories to compare. Indices, not names like a panel's marks:
    /// results never reorder while the popup is open, and two results can
    /// share a file name.
    pub marked: HashSet<usize>,
    /// The search currently running on a background thread --
    /// `Some` only during `FindFilePhase::Searching`, `None` in every
    /// other phase (set by `run_search`, cleared by
    /// `background::poll_pending_find_file_search` once it finishes, or
    /// dropped -- after being told to cancel first -- when `Esc` closes
    /// the popup mid-search).
    pub pending: Option<PendingSearch>,
    /// Wall-clock duration of the search behind `results`, shown in the popup
    /// (read when the poll sees it finish). `None` before the first search.
    pub search_duration: Option<Duration>,
    /// Whether a `find_file_max_results`/`find_file_max_visited` cap cut the
    /// results short -- the popup then shows "+" on the count, since a search
    /// hitting exactly 200 looked complete. Not set by an `Esc` cancel.
    /// History: docs/history/find-file-search.md.
    pub results_capped: bool,
    /// Feedback from the last `Ctrl+S` export, shown under the results
    /// list until the next export attempt (success or failure). Kept in
    /// the popup rather than a notice, next to the list it's about.
    /// `(label, detail)` — `("Exported to:", "<path>")` or
    /// `("Export failed:", "<error>")` — rendered on two separate
    /// lines rather than one, since a real Downloads path is easily
    /// wide enough to blow past the popup's width on one line.
    pub export_message: Option<(String, String)>,
}


impl FindFileState {
    pub fn new() -> Self {
        Self {
            phase: FindFilePhase::Typing,
            query: TextField::new(),
            content_query: TextField::new(),
            active_field: FindFileField::Name,
            name_history_index: None,
            content_history_index: None,
            results: Vec::new(),
            selected: 0,
            marked: HashSet::new(),
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
    /// `Editor::search_history_up` exactly, just against `query`/
    /// `name_history_index` instead of the editor's own search box.
    pub fn name_history_up(&mut self, history: &[String]) {
        Self::history_up(history, &mut self.name_history_index, &mut self.query);
    }

    /// `Down` -- the other half of `name_history_up`: steps back toward
    /// the most recent entry, and past it clears the field entirely. A
    /// no-op while not currently browsing history at all.
    pub fn name_history_down(&mut self, history: &[String]) {
        Self::history_down(history, &mut self.name_history_index, &mut self.query);
    }

    /// Same as `name_history_up`, for `content_query`/`content_history_index`.
    pub fn content_history_up(&mut self, history: &[String]) {
        Self::history_up(history, &mut self.content_history_index, &mut self.content_query);
    }

    /// Same as `name_history_down`, for `content_query`/`content_history_index`.
    pub fn content_history_down(&mut self, history: &[String]) {
        Self::history_down(history, &mut self.content_history_index, &mut self.content_query);
    }

    /// Shared mechanics for `name_history_up`/`content_history_up`. The
    /// selection is always dropped -- a stale one could point past the
    /// end of a shorter recalled entry.
    fn history_up(history: &[String], index: &mut Option<usize>, field: &mut TextField) {
        field.clear_selection();
        if history.is_empty() {
            return;
        }
        let next_index = match *index {
            None => history.len() - 1,
            Some(current) => current.saturating_sub(1),
        };
        *index = Some(next_index);
        field.set_text(history[next_index].clone());
    }

    /// Shared mechanics for `name_history_down`/`content_history_down`.
    fn history_down(history: &[String], index: &mut Option<usize>, field: &mut TextField) {
        field.clear_selection();
        let Some(current) = *index else {
            return;
        };
        if current + 1 < history.len() {
            *index = Some(current + 1);
            field.set_text(history[current + 1].clone());
        } else {
            *index = None;
            field.clear();
        }
    }

    /// `Shift+Down` on the results list -- toggles the mark on the
    /// current row, then moves down, same "paint a block one row at a
    /// time on repeated presses" convention `Panel::toggle_mark_move_down`
    /// already uses.
    pub fn toggle_mark_move_down(&mut self) {
        self.toggle_mark_at(self.selected);
        if self.selected + 1 < self.results.len() {
            self.selected += 1;
        }
    }

    /// `Shift+Up` -- mirror of `toggle_mark_move_down`.
    pub fn toggle_mark_move_up(&mut self) {
        self.toggle_mark_at(self.selected);
        self.selected = self.selected.saturating_sub(1);
    }

    /// Toggles the mark on `results[index]` -- a no-op for an
    /// out-of-range index (an empty results list), unlike
    /// `Panel::toggle_mark_at` there's no synthetic `..` entry here that
    /// needs its own separate exclusion.
    fn toggle_mark_at(&mut self, index: usize) {
        if index >= self.results.len() {
            return;
        }
        if !self.marked.remove(&index) {
            self.marked.insert(index);
        }
    }

    /// The two marked results, in ascending index order -- `None` unless
    /// *exactly* two are marked, matching
    /// `command_line::browsing::compare_targets`'s own "two marked
    /// entries or nothing" convention for the same Alt+F5 compare
    /// action. Ascending order (rather than mark/unmark order) keeps
    /// which side is "left" and which is "right" predictable regardless
    /// of which of the two the user happened to mark first.
    pub fn two_marked_results(&self) -> Option<(PathBuf, PathBuf)> {
        let [left, right] = self.marked_results().try_into().ok()?;
        Some((left, right))
    }

    /// Every marked result, in ascending index order.
    pub fn marked_results(&self) -> Vec<PathBuf> {
        let mut indices: Vec<usize> = self.marked.iter().copied().collect();
        indices.sort_unstable();
        indices.into_iter().filter_map(|index| self.results.get(index).cloned()).collect()
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

        assert_eq!(state.query.text(), "recent.txt");
        assert_eq!(state.query.cursor(), "recent.txt".chars().count());
    }

    #[test]
    fn name_history_up_steps_further_back_on_repeated_presses() {
        let mut state = FindFileState::new();
        let history = vec!["old.txt".to_string(), "recent.txt".to_string()];

        state.name_history_up(&history);
        state.name_history_up(&history);

        assert_eq!(state.query.text(), "old.txt");
    }

    #[test]
    fn name_history_up_stops_at_the_oldest_entry() {
        let mut state = FindFileState::new();
        let history = vec!["only.txt".to_string()];

        state.name_history_up(&history);
        state.name_history_up(&history);

        assert_eq!(state.query.text(), "only.txt");
    }

    #[test]
    fn name_history_up_is_a_noop_with_empty_history() {
        let mut state = FindFileState::new();
        state.query.set_text("untouched");

        state.name_history_up(&[]);

        assert_eq!(state.query.text(), "untouched");
    }

    #[test]
    fn name_history_down_steps_back_toward_the_most_recent_entry() {
        let mut state = FindFileState::new();
        let history = vec!["old.txt".to_string(), "recent.txt".to_string()];
        state.name_history_up(&history);
        state.name_history_up(&history); // now on "old.txt"

        state.name_history_down(&history);

        assert_eq!(state.query.text(), "recent.txt");
    }

    #[test]
    fn name_history_down_past_the_newest_entry_clears_the_field() {
        let mut state = FindFileState::new();
        let history = vec!["recent.txt".to_string()];
        state.name_history_up(&history);

        state.name_history_down(&history);

        assert_eq!(state.query.text(), "");
        assert_eq!(state.name_history_index, None);
    }

    #[test]
    fn name_history_down_is_a_noop_when_not_currently_browsing() {
        let mut state = FindFileState::new();
        state.query.set_text("still typing");

        state.name_history_down(&["recalled.txt".to_string()]);

        assert_eq!(state.query.text(), "still typing");
    }

    /// `content_history_up`/`_down` are the same mechanics as
    /// `name_history_up`/`_down`, just against the other field --
    /// confirms they're wired to `content_query`/`content_cursor`/
    /// `content_history_index`, not accidentally sharing the name
    /// field's own state.
    #[test]
    fn content_history_up_and_down_operate_on_the_content_field_independently() {
        let mut state = FindFileState::new();
        state.query.set_text("name field untouched");
        let history = vec!["needle".to_string()];

        state.content_history_up(&history);

        assert_eq!(state.content_query.text(), "needle");
        assert_eq!(state.query.text(), "name field untouched", "browsing the content field's history shouldn't touch the name field");

        state.content_history_down(&history);

        assert_eq!(state.content_query.text(), "");
    }

    mod marking_tests {
        use std::path::PathBuf;

        use super::*;

        fn state_with_results(count: usize) -> FindFileState {
            let mut state = FindFileState::new();
            state.results = (0..count).map(|i| PathBuf::from(format!("file_{i}.txt"))).collect();
            state
        }

        #[test]
        fn shift_down_marks_the_current_row_then_moves_down() {
            let mut state = state_with_results(3);

            state.toggle_mark_move_down();

            assert_eq!(state.marked, HashSet::from([0]));
            assert_eq!(state.selected, 1);
        }

        #[test]
        fn shift_up_marks_the_current_row_then_moves_up() {
            let mut state = state_with_results(3);
            state.selected = 2;

            state.toggle_mark_move_up();

            assert_eq!(state.marked, HashSet::from([2]));
            assert_eq!(state.selected, 1);
        }

        #[test]
        fn repeated_shift_down_paints_a_block_and_stops_at_the_last_row() {
            let mut state = state_with_results(3);

            state.toggle_mark_move_down();
            state.toggle_mark_move_down();
            state.toggle_mark_move_down();

            assert_eq!(state.marked, HashSet::from([0, 1, 2]), "the last row should still get marked even though there's nowhere further to move");
            assert_eq!(state.selected, 2, "selection shouldn't run off the end of the list");
        }

        #[test]
        fn marking_the_same_row_twice_unmarks_it() {
            let mut state = state_with_results(3);

            state.toggle_mark_move_down();
            state.selected = 0;
            state.toggle_mark_move_down();

            assert!(state.marked.is_empty(), "toggling the same row again should unmark it");
        }

        #[test]
        fn two_marked_results_returns_none_unless_exactly_two_are_marked() {
            let mut state = state_with_results(3);
            assert_eq!(state.two_marked_results(), None, "nothing marked yet");

            state.toggle_mark_move_down(); // marks row 0
            assert_eq!(state.two_marked_results(), None, "only one marked");

            state.toggle_mark_move_down(); // marks row 1
            assert_eq!(
                state.two_marked_results(),
                Some((PathBuf::from("file_0.txt"), PathBuf::from("file_1.txt")))
            );

            state.selected = 2;
            state.toggle_mark_move_down(); // marks row 2, now three marked
            assert_eq!(state.two_marked_results(), None, "three marked is no longer exactly two");
        }

        #[test]
        fn two_marked_results_are_returned_in_ascending_index_order_regardless_of_mark_order() {
            let mut state = state_with_results(3);
            state.selected = 2;
            state.toggle_mark_move_up(); // marks row 2 first, moves to row 1
            state.toggle_mark_move_up(); // marks row 1, moves to row 0

            assert_eq!(
                state.two_marked_results(),
                Some((PathBuf::from("file_1.txt"), PathBuf::from("file_2.txt"))),
                "ascending by index, not by which one was marked first"
            );
        }
    }
}
