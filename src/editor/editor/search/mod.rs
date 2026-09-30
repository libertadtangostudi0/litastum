use crossterm::event::KeyEvent;
use edtui::actions::{Execute, SwitchMode};
use edtui::{EditorMode, Index2};

use crate::text_field::{EditOutcome, TextField};

use super::Editor;

mod session;

use session::SearchSession;

/// The open `Ctrl+F` box: a standard single-line text field (the same
/// keys as Find file's fields) plus the matching state for whatever it
/// currently holds. Requested directly: the box used to be append/
/// Backspace-only, with no way to select part of the query or fix a
/// typo in the middle of it.
pub(in crate::editor::editor) struct SearchBox {
    field: TextField,
    session: SearchSession,
}


impl SearchBox {
    fn new(start_cursor: Index2) -> Self {
        Self { field: TextField::new(), session: SearchSession::new(start_cursor) }
    }
}


impl Editor {
    /// Whether the `Ctrl+F` box has keyboard focus (`EditorMode::Search` is
    /// the flag; matching is our `SearchSession`). Distinct from
    /// `search_box_open`: a click in the text takes focus while the box stays
    /// open, as in VS Code.
    pub fn is_searching(&self) -> bool {
        self.state.mode == EditorMode::Search
    }

    /// Whether the `Ctrl+F` box is open at all, focused or not -- drawn,
    /// and its current match highlighted, the whole time.
    pub fn search_box_open(&self) -> bool {
        self.search.is_some()
    }

    /// The search box's current query text, for rendering the popup.
    pub fn search_query(&self) -> String {
        self.search_field().map(|field| field.text().to_string()).unwrap_or_default()
    }

    /// The search box's text field (text, cursor, selection), `None`
    /// while the box is closed -- for rendering it.
    pub fn search_field(&self) -> Option<&TextField> {
        self.search.as_ref().map(|search_box| &search_box.field)
    }

    /// Whether the box's own cursor sits right after its last character
    /// -- where `End` accepts the history suggestion instead of moving
    /// (`editor_keymap::handle_search_key`).
    pub fn search_cursor_at_end(&self) -> bool {
        self.search_field().is_none_or(|field| field.cursor() == field.text().chars().count())
    }

    /// The currently selected match as an inclusive `(start, end)` span
    /// on one row, for `view()` to highlight -- `None` while the box is
    /// closed, the query is empty, or nothing matches.
    pub(super) fn search_match_span(&self) -> Option<(Index2, Index2)> {
        let session = &self.search.as_ref()?.session;
        let start = session.selected_match()?;
        Some((start, Index2::new(start.row, start.col + session.pattern_len().saturating_sub(1))))
    }

    /// `Ctrl+F`: opens the search box, anchored at the cursor's current
    /// position (what `stop_search` below restores if the box is
    /// cancelled with nothing found). If it's already open but the text
    /// has focus, gives focus back to it instead, the way VS Code's own
    /// `Ctrl+F` does: the query stays, fully selected so typing replaces
    /// it, and searching continues from wherever the caret now is.
    pub fn start_search(&mut self) {
        match &mut self.search {
            Some(search_box) => {
                search_box.field.select_all();
                search_box.session.set_start_cursor(self.state.cursor);
            }
            None => self.search = Some(SearchBox::new(self.state.cursor)),
        }
        SwitchMode(EditorMode::Search).execute(&mut self.state);
        self.search_history_index = None;
    }

    /// Takes keyboard focus away from the box and gives it to the text,
    /// leaving the box open and its match highlighted -- a click in the
    /// text (`Editor::mouse`). The caret stays wherever it is.
    pub(super) fn blur_search_box(&mut self) {
        if self.is_searching() {
            SwitchMode(super::starting_mode(self.keymap_mode)).execute(&mut self.state);
        }
    }

    /// `Esc` while the text has focus and the box is open -- closes the
    /// box and leaves the caret exactly where it is (unlike `Esc` inside
    /// the box, `stop_search`, which lands the caret after the match).
    pub fn close_search_box(&mut self) {
        self.search = None;
    }

    /// Keeps the open box's matches in step with an edited buffer -- the
    /// text can be edited while the box stays open (focus in the text),
    /// and every stored match position is stale after that. See
    /// `Editor::buffer_changed`.
    pub(super) fn refresh_search_matches(&mut self) {
        if let Some(search_box) = &mut self.search {
            search_box.session.rebuild(&self.state.lines);
        }
    }

    /// One editing key in the search box (`TextField::apply_key`). A text
    /// change updates the matches, jumps to the first one and ends history
    /// browsing. Returns whether it was an editing key.
    pub fn search_edit_key(&mut self, key: KeyEvent) -> bool {
        let lines = &self.state.lines;
        let search_box = self.search.get_or_insert_with(|| SearchBox::new(self.state.cursor));
        let outcome = search_box.field.apply_key(key);
        if outcome == EditOutcome::TextChanged {
            search_box.session.set_pattern(lines, search_box.field.text());
            self.jump_to_first_match();
            self.search_history_index = None;
        }
        outcome != EditOutcome::Unhandled
    }

    /// Types `c` at the box's cursor -- `search_edit_key` for a plain
    /// character.
    #[cfg(test)]
    pub fn search_push_char(&mut self, c: char) {
        self.search_edit_key(KeyEvent::new(crossterm::event::KeyCode::Char(c), crossterm::event::KeyModifiers::NONE));
    }

    /// `Enter` in the box, or `F3` from the text -- jumps to the next
    /// match after the caret, VS Code's own convention (reported
    /// directly: `Up`/`Down` was tried for this first and reported wrong
    /// -- those are for browsing *history* instead, below, the same way a
    /// shell's own `Up`/`Down` work on the command being typed).
    pub fn search_next(&mut self) {
        let caret = self.state.cursor;
        if let Some(start) = self.search.as_mut().and_then(|search_box| search_box.session.select_next_after(caret)) {
            self.state.cursor = start;
        }
    }

    /// `Shift+Enter` / `Shift+F3` -- the previous match before the caret,
    /// the other half of the convention `search_next` follows.
    pub fn search_previous(&mut self) {
        let caret = self.state.cursor;
        if let Some(start) = self.search.as_mut().and_then(|search_box| search_box.session.select_previous_before(caret)) {
            self.state.cursor = start;
        }
    }

    /// `Esc`: closes the box. On a match, the cursor goes one past its last
    /// character; with no query or no match, back to where the search started.
    /// Not onto the last character: with no selection there's no bar-cursor
    /// shift, and "lso" landed visually between 's' and 'o'. History: docs/history/editor-keymap.md.
    pub fn stop_search(&mut self) {
        let query = self.search_query();
        if !query.is_empty() && self.cursor_sits_on_a_real_match(&query) {
            self.state.cursor.col += query.chars().count();
        } else if let Some(search_box) = &self.search {
            self.state.cursor = search_box.session.start_cursor();
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
        if let Some(start) = self.search.as_mut().and_then(|search_box| search_box.session.select_first_from_start()) {
            self.state.cursor = start;
        }
    }

    /// `Up`: the previous `history` entry, shell-style -- not the previous
    /// match (that's `Shift+Enter`/`Shift+F3`). No-op at the oldest entry or
    /// with no history.
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

    /// `Down`: back toward the newest entry; past it, an empty query. No-op
    /// when not browsing history.
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
    /// leaves history-browsing, same reasoning as `search_edit_key`
    /// (this is a fresh, explicit choice of query, not a step through
    /// `Up`/`Down`'s own separate history walk).
    pub fn accept_search_suggestion(&mut self, suggestion: &str) {
        self.replace_search_query(suggestion);
        self.search_history_index = None;
    }

    /// Shared mechanics for `accept_search_suggestion` and
    /// `search_history_up`/`_down` above -- the whole text replaced, the
    /// cursor at its end, no selection, and the matches synced to it.
    fn replace_search_query(&mut self, new_query: &str) {
        let lines = &self.state.lines;
        let search_box = self.search.get_or_insert_with(|| SearchBox::new(self.state.cursor));
        search_box.field.set_text(new_query);
        search_box.session.set_pattern(lines, new_query);
        self.jump_to_first_match();
    }
}
