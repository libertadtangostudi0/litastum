use edtui::actions::{Execute, SwitchMode};
use edtui::{EditorMode, Index2};

use super::Editor;

mod session;

pub(super) use session::SearchSession;

impl Editor {
    /// `Ctrl+F` -- whether the built-in search box is currently open.
    /// `EditorMode::Search` is still the flag for this (it's what
    /// `editor_keymap::handle_search_key` routes on, and `edtui` itself
    /// draws nothing extra for it while its own `SearchState` stays
    /// empty), but the matching itself is this app's own
    /// `SearchSession` -- see its doc comment for why `edtui`'s search
    /// couldn't be kept.
    pub fn is_searching(&self) -> bool {
        self.state.mode == EditorMode::Search
    }

    /// The search box's current query text, for rendering the popup.
    pub fn search_query(&self) -> String {
        self.search.as_ref().map(SearchSession::pattern).unwrap_or_default()
    }

    /// The currently selected match as an inclusive `(start, end)` span
    /// on one row, for `view()` to highlight -- `None` while the box is
    /// closed, the query is empty, or nothing matches.
    pub(super) fn search_match_span(&self) -> Option<(Index2, Index2)> {
        if !self.is_searching() {
            return None;
        }
        let session = self.search.as_ref()?;
        let start = session.selected_match()?;
        Some((start, Index2::new(start.row, start.col + session.pattern_len().saturating_sub(1))))
    }

    /// Opens the search box, anchored at the cursor's current position
    /// (what `stop_search` below restores if the box is cancelled with
    /// nothing found).
    pub fn start_search(&mut self) {
        self.search = Some(SearchSession::new(self.state.cursor));
        SwitchMode(EditorMode::Search).execute(&mut self.state);
        self.search_history_index = None;
    }

    /// One typed character into the search box -- re-filters the matches
    /// immediately and jumps to the first one, the same find-as-you-type
    /// feel as the command line's own always-live autosuggestion. Also
    /// leaves history-browsing (`Up`/`Down`, below) -- typing means the
    /// query is being edited fresh again, not still showing whatever
    /// history entry `Up`/`Down` last recalled.
    pub fn search_push_char(&mut self, c: char) {
        let lines = &self.state.lines;
        self.search.get_or_insert_with(|| SearchSession::new(self.state.cursor)).push(lines, c);
        self.jump_to_first_match();
        self.search_history_index = None;
    }

    /// `Backspace` in the search box -- pops the *last* character (no
    /// mid-string cursor to delete from, matching the box's own "just an
    /// input field" scope for now) and jumps to the first match of the
    /// now-shorter query, same as typing does. `edtui`'s own version
    /// left the cursor (and a possibly stale match index) wherever the
    /// longer query had put it. Also leaves history-browsing, same
    /// reasoning as `search_push_char`.
    pub fn search_pop_char(&mut self) {
        if let Some(session) = &mut self.search {
            session.pop();
        }
        self.jump_to_first_match();
        self.search_history_index = None;
    }

    /// `Enter` -- jumps to the next match, VS Code's own `Ctrl+F`
    /// convention (reported directly: `Up`/`Down` was tried for this
    /// first and reported wrong -- those are for browsing *history*
    /// instead, below, the same way a shell's own `Up`/`Down` work on
    /// the command being typed, not on some other piece of state).
    pub fn search_next(&mut self) {
        if let Some(start) = self.search.as_mut().and_then(SearchSession::select_next) {
            self.state.cursor = start;
        }
    }

    /// `Shift+Enter` -- jumps to the previous match, the other half of
    /// the VS Code convention `search_next` follows.
    pub fn search_previous(&mut self) {
        if let Some(start) = self.search.as_mut().and_then(SearchSession::select_previous) {
            self.state.cursor = start;
        }
    }

    /// `Esc` -- closes the search box. Requested directly: if the
    /// cursor is currently sitting on a real match, leave it right
    /// *after* the match's own last character (the ordinary "next
    /// character you'd type" position, same as where a plain typing
    /// cursor always sits) -- rather than reverting to wherever the
    /// cursor was before the box opened. Only reverts when there's
    /// genuinely nothing to land on: an empty query, or no match.
    ///
    /// **One past the match's last character, not directly on it** --
    /// reported directly against a real search ("lso" landed the cursor
    /// visually *between* 's' and the final 'o', not after it). This
    /// codebase's own "cursor sits on the last *selected* character"
    /// convention (`bindings::word_select`'s forward-selection landing,
    /// the vertical-shift-select fix, ...) only reads right because
    /// `Editor::cursor_screen_position` shifts the rendered bar one
    /// column past whatever cell `state.cursor` names *while a selection
    /// is active* -- closing the search box leaves `state.selection`
    /// untouched (`None`), so that shift never fires here, and landing on
    /// the match's own last character would visually read as stopping
    /// one short of it, same trap that convention exists to avoid in the
    /// first place.
    pub fn stop_search(&mut self) {
        let query = self.search_query();
        if !query.is_empty() && self.cursor_sits_on_a_real_match(&query) {
            self.state.cursor.col += query.chars().count();
        } else if let Some(session) = &self.search {
            self.state.cursor = session.start_cursor();
        }
        SwitchMode(EditorMode::Insert).execute(&mut self.state);
        self.search = None;
    }

    /// Whether the cursor is currently sitting exactly on the *start* of
    /// a real occurrence of `query` in the buffer, checked directly
    /// against the buffer with a plain character peek.
    fn cursor_sits_on_a_real_match(&self, query: &str) -> bool {
        query.chars().enumerate().all(|(offset, expected)| {
            let position = Index2 { row: self.state.cursor.row, col: self.state.cursor.col + offset };
            self.state.lines.get(position).is_some_and(|found| found.to_lowercase().eq(expected.to_lowercase()))
        })
    }

    fn jump_to_first_match(&mut self) {
        if let Some(start) = self.search.as_mut().and_then(SearchSession::select_first_from_start) {
            self.state.cursor = start;
        }
    }

    /// `Up` -- recalls the *previous* entry in `history` (a shell's own
    /// `Up`-arrow convention: first press shows the most recent past
    /// query, each further press steps one entry further back), rather
    /// than moving between matches of the *current* query -- see
    /// `search_next`/`search_previous` above for why those, not
    /// `Up`/`Down`, are what the report actually asked for that. A
    /// no-op with nothing to recall (`history` empty, or already at the
    /// oldest entry).
    pub fn search_history_up(&mut self, history: &[String]) {
        if history.is_empty() {
            return;
        }
        let next_index = match self.search_history_index {
            None => history.len() - 1,
            Some(index) => index.saturating_sub(1),
        };
        self.search_history_index = Some(next_index);
        self.replace_search_query(&history[next_index].clone());
    }

    /// `Down` -- the other half of `search_history_up`: steps back
    /// *toward* the most recent entry, and past it clears the query
    /// entirely (the shell convention's own "back to your own
    /// not-yet-recalled line," simplified here to just "empty," since
    /// this box has no separate "what was I typing before I started
    /// browsing" state to restore -- matches its own "just an input
    /// field for now" scope). A no-op while not currently browsing
    /// history at all (`Up` was never pressed, or a keystroke since
    /// already cleared it -- see `search_push_char`/`search_pop_char`).
    pub fn search_history_down(&mut self, history: &[String]) {
        let Some(index) = self.search_history_index else {
            return;
        };
        if index + 1 < history.len() {
            self.search_history_index = Some(index + 1);
            self.replace_search_query(&history[index + 1].clone());
        } else {
            self.search_history_index = None;
            self.replace_search_query("");
        }
    }

    /// Replaces the search box's current query with `suggestion` in
    /// full (accepting the ghost-text history suggestion, `End`) --
    /// leaves history-browsing, same reasoning as `search_push_char`
    /// (this is a fresh, explicit choice of query, not a step through
    /// `Up`/`Down`'s own separate history walk).
    pub fn accept_search_suggestion(&mut self, suggestion: &str) {
        self.replace_search_query(suggestion);
        self.search_history_index = None;
    }

    /// Shared mechanics for `accept_search_suggestion` and
    /// `search_history_up`/`_down` above -- a fresh session from the same
    /// start position, fed `new_query` one character at a time (only the
    /// first character scans the buffer, see `SearchSession`).
    fn replace_search_query(&mut self, new_query: &str) {
        let start = self.search.as_ref().map_or(self.state.cursor, SearchSession::start_cursor);
        let mut session = SearchSession::new(start);
        for c in new_query.chars() {
            session.push(&self.state.lines, c);
        }
        self.search = Some(session);
        self.jump_to_first_match();
    }
}
