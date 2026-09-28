use crossterm::event::KeyEvent;
use edtui::actions::{Execute, SwitchMode};
use edtui::{EditorMode, Index2};

use crate::text_field::{self, EditOutcome};

use super::Editor;

mod session;

use session::SearchSession;

/// The open `Ctrl+F` box: a real single-line text field (its own
/// cursor and selection, the same keys as Find file's fields --
/// `text_field::apply_edit_key`) plus the matching state for whatever it
/// currently holds. Requested directly: the box used to be append/
/// Backspace-only, with no way to select part of the query or fix a
/// typo in the middle of it.
pub(in crate::editor::editor) struct SearchBox {
    text: String,
    /// Character index into `text`, same convention as every other
    /// `text_field` user.
    cursor: usize,
    selection_anchor: Option<usize>,
    session: SearchSession,
}


impl SearchBox {
    fn new(start_cursor: Index2) -> Self {
        Self { text: String::new(), cursor: 0, selection_anchor: None, session: SearchSession::new(start_cursor) }
    }
}


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
        self.search.as_ref().map(|search_box| search_box.text.clone()).unwrap_or_default()
    }

    /// The search box's own cursor, as a character index into
    /// `search_query()`.
    pub fn search_cursor(&self) -> usize {
        self.search.as_ref().map_or(0, |search_box| search_box.cursor)
    }

    /// The search box's own selected character range (`start..end`,
    /// end exclusive), if any -- for rendering it highlighted.
    pub fn search_selection(&self) -> Option<(usize, usize)> {
        let search_box = self.search.as_ref()?;
        search_box.selection_anchor.map(|anchor| text_field::selection_range(anchor, search_box.cursor))
    }

    /// Whether the box's own cursor sits right after its last character
    /// -- where `End` accepts the history suggestion instead of moving
    /// (`editor_keymap::handle_search_key`).
    pub fn search_cursor_at_end(&self) -> bool {
        self.search.as_ref().is_none_or(|search_box| search_box.cursor == search_box.text.chars().count())
    }

    /// The currently selected match as an inclusive `(start, end)` span
    /// on one row, for `view()` to highlight -- `None` while the box is
    /// closed, the query is empty, or nothing matches.
    pub(super) fn search_match_span(&self) -> Option<(Index2, Index2)> {
        if !self.is_searching() {
            return None;
        }
        let session = &self.search.as_ref()?.session;
        let start = session.selected_match()?;
        Some((start, Index2::new(start.row, start.col + session.pattern_len().saturating_sub(1))))
    }

    /// Opens the search box, anchored at the cursor's current position
    /// (what `stop_search` below restores if the box is cancelled with
    /// nothing found).
    pub fn start_search(&mut self) {
        self.search = Some(SearchBox::new(self.state.cursor));
        SwitchMode(EditorMode::Search).execute(&mut self.state);
        self.search_history_index = None;
    }

    /// One editing key in the search box (`text_field::apply_edit_key` --
    /// typing, Backspace/Delete, character/word selection with
    /// `Shift`/`Ctrl+Shift` + arrows, `Home`/`End`, ...). Whenever the
    /// text itself changes, the matches follow immediately and the
    /// cursor jumps to the first one (find-as-you-type), and history
    /// browsing ends -- editing means the query is being written fresh
    /// again, not still showing whatever `Up`/`Down` last recalled.
    /// Returns whether the key was an editing key at all.
    pub fn search_edit_key(&mut self, key: KeyEvent) -> bool {
        let lines = &self.state.lines;
        let search_box = self.search.get_or_insert_with(|| SearchBox::new(self.state.cursor));
        let outcome = text_field::apply_edit_key(&mut search_box.text, &mut search_box.cursor, &mut search_box.selection_anchor, key);
        if outcome == EditOutcome::TextChanged {
            search_box.session.set_pattern(lines, &search_box.text);
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

    /// `Enter` -- jumps to the next match, VS Code's own `Ctrl+F`
    /// convention (reported directly: `Up`/`Down` was tried for this
    /// first and reported wrong -- those are for browsing *history*
    /// instead, below, the same way a shell's own `Up`/`Down` work on
    /// the command being typed, not on some other piece of state).
    pub fn search_next(&mut self) {
        if let Some(start) = self.search.as_mut().and_then(|search_box| search_box.session.select_next()) {
            self.state.cursor = start;
        }
    }

    /// `Shift+Enter` -- jumps to the previous match, the other half of
    /// the VS Code convention `search_next` follows.
    pub fn search_previous(&mut self) {
        if let Some(start) = self.search.as_mut().and_then(|search_box| search_box.session.select_previous()) {
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
    /// browsing" state to restore). A no-op while not currently browsing
    /// history at all (`Up` was never pressed, or an edit since already
    /// cleared it -- see `search_edit_key`).
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
        search_box.text = new_query.to_string();
        search_box.cursor = new_query.chars().count();
        search_box.selection_anchor = None;
        search_box.session.set_pattern(lines, new_query);
        self.jump_to_first_match();
    }
}
