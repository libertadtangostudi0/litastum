use edtui::actions::search::StartSearch;
use edtui::actions::{AppendCharToSearch, Execute, FindNext, FindPrevious, RemoveCharFromSearch, StopSearch, SwitchMode};
use edtui::{EditorMode, Index2};

use super::Editor;

impl Editor {
    /// `Ctrl+F` -- whether the built-in search box is currently open.
    /// `edtui` already ships a complete search mechanism of its own
    /// (`EditorMode::Search`, `actions::search::*`) -- rather than
    /// hand-rolling match-finding and a second selection-like highlight
    /// (which would duplicate what `EditorView::render` already does for
    /// `state.search` for free, see `edtui-0.11.7/src/view.rs`), this
    /// and the methods below just drive that mechanism directly, the
    /// same way `standard_key_handler`'s own table drives ordinary
    /// motion/editing actions. `editor_keymap.rs::handle_search_key`
    /// intercepts every key ahead of the normal table while this is
    /// `true`, exactly like an active word-select drag never reaches
    /// `Editor::input` either.
    pub fn is_searching(&self) -> bool {
        self.state.mode == EditorMode::Search
    }

    /// The search box's current query text, for rendering the popup.
    pub fn search_query(&self) -> String {
        self.state.search_pattern()
    }

    /// Opens the search box, anchored at the cursor's current position
    /// (`StartSearch` records it as `search.start_cursor`, the position
    /// `stop_search` below restores if the box is cancelled with nothing
    /// found).
    pub fn start_search(&mut self) {
        StartSearch.execute(&mut self.state);
        SwitchMode(EditorMode::Search).execute(&mut self.state);
        self.search_history_index = None;
    }

    /// One typed character into the search box -- re-runs the search
    /// immediately (`AppendCharToSearch`'s own `execute`), same
    /// find-as-you-type feel as the command line's own always-live
    /// autosuggestion. Also leaves history-browsing (`Up`/`Down`, below)
    /// -- typing means the query is being edited fresh again, not still
    /// showing whatever history entry `Up`/`Down` last recalled.
    pub fn search_push_char(&mut self, c: char) {
        AppendCharToSearch(c).execute(&mut self.state);
        self.search_history_index = None;
    }

    /// `Backspace` in the search box -- `edtui`'s own action only ever
    /// pops the *last* character (no mid-string cursor to delete from,
    /// matching the box's own "just an input field" scope for now).
    /// Also leaves history-browsing, same reasoning as `search_push_char`.
    pub fn search_pop_char(&mut self) {
        RemoveCharFromSearch.execute(&mut self.state);
        self.search_history_index = None;
    }

    /// `Enter` -- jumps to the next match, VS Code's own `Ctrl+F`
    /// convention (reported directly: `Up`/`Down` was tried for this
    /// first and reported wrong -- those are for browsing *history*
    /// instead, below, the same way a shell's own `Up`/`Down` work on
    /// the command being typed, not on some other piece of state).
    pub fn search_next(&mut self) {
        FindNext.execute(&mut self.state);
    }

    /// `Shift+Enter` -- jumps to the previous match, the other half of
    /// the VS Code convention `search_next` follows.
    pub fn search_previous(&mut self) {
        FindPrevious.execute(&mut self.state);
    }

    /// `Esc` -- closes the search box. Requested directly: if the
    /// cursor is currently sitting on a real match, leave it right
    /// *after* the match's own last character (the ordinary "next
    /// character you'd type" position, same as where a plain typing
    /// cursor always sits) -- rather than `StopSearch`'s own default of
    /// reverting to wherever the cursor was before the box opened. Only
    /// reverts (still via `StopSearch`) when there's genuinely nothing
    /// to land on: an empty query, or `edtui`'s own
    /// `AppendCharToSearch`/`RemoveCharFromSearch` actions leaving the
    /// cursor on a *stale* position that no longer matches the current
    /// query at all (they only ever re-jump the cursor on
    /// `AppendCharToSearch`, never on a backspace -- see
    /// `cursor_sits_on_a_real_match`'s own doc comment).
    ///
    /// **One past the match's last character, not directly on it** --
    /// reported directly against a real search ("lso" landed the cursor
    /// visually *between* 's' and the final 'o', not after it). This
    /// codebase's own "cursor sits on the last *selected* character"
    /// convention (`bindings::word_select`'s forward-selection landing,
    /// the vertical-shift-select fix earlier this session, ...) only
    /// reads right because `Editor::cursor_screen_position` shifts the
    /// rendered bar one column past whatever cell `state.cursor` names
    /// *while a selection is active* -- closing the search box also
    /// leaves `state.selection` untouched (`None`), so that shift never
    /// fires here, and landing on the match's own last character would
    /// visually read as stopping one short of it, same trap that
    /// convention exists to avoid in the first place.
    pub fn stop_search(&mut self) {
        let query = self.state.search_pattern();
        if !query.is_empty() && self.cursor_sits_on_a_real_match(&query) {
            self.state.cursor.col += query.chars().count();
            SwitchMode(EditorMode::Insert).execute(&mut self.state);
        } else {
            StopSearch.execute(&mut self.state);
            SwitchMode(EditorMode::Insert).execute(&mut self.state);
        }
    }

    /// Whether the cursor is currently sitting exactly on the *start* of
    /// a real occurrence of `query` in the buffer -- the only way
    /// `stop_search` above has to tell "there's a live match here" from
    /// "the cursor is stale, left over from before the query last
    /// changed," since `edtui`'s own `SearchState` (pattern/matches/
    /// selected index) is `pub(crate)`, entirely unreachable from here;
    /// `search_pattern()` is the *only* public window into it. Checked
    /// directly against the buffer instead, the same single-character
    /// peek (`state.lines.get(Index2)`) `bindings::word_select`'s own
    /// anchor-trim fixes already use, rather than trying to reconstruct
    /// `edtui`'s internal match bookkeeping some other way.
    fn cursor_sits_on_a_real_match(&self, query: &str) -> bool {
        query.chars().enumerate().all(|(offset, expected)| {
            let position = Index2 { row: self.state.cursor.row, col: self.state.cursor.col + offset };
            self.state.lines.get(position) == Some(&expected)
        })
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
    /// `search_history_up`/`_down` above -- there's no `edtui` action to
    /// set the whole search pattern at once, only append/pop one
    /// character, so this pops the existing query back to nothing and
    /// re-appends `new_query` one character at a time through those same
    /// public actions, exactly as if it had been typed.
    fn replace_search_query(&mut self, new_query: &str) {
        let current_len = self.state.search_pattern().chars().count();
        for _ in 0..current_len {
            RemoveCharFromSearch.execute(&mut self.state);
        }
        for c in new_query.chars() {
            AppendCharToSearch(c).execute(&mut self.state);
        }
    }
}
