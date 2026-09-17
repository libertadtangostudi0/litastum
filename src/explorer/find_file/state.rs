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
    /// Which of `query`/`content_query` `Tab` and typed characters
    /// currently reach.
    pub active_field: FindFileField,
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
            content_query: String::new(),
            content_cursor: 0,
            active_field: FindFileField::Name,
            results: Vec::new(),
            selected: 0,
            pending: None,
            search_duration: None,
            results_capped: false,
            export_message: None,
        }
    }
}
