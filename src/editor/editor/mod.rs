use std::fs;
use std::io;
use std::path::PathBuf;

use edtui::actions::motion::{MoveToFirstRow, MoveToLastRow};
use edtui::actions::{Chainable, Execute, MoveToEndOfLine, MoveToStartOfLine, SwitchMode};
use edtui::syntect::highlighting::Theme as SynTheme;
use edtui::{EditorEventHandler, EditorMode, EditorState, Highlight, Index2, Lines};
use tracing::{debug, warn};

use super::bindings::standard_key_handler;
use super::clipboard::OsClipboardBridge;
use super::keymap_mode::EditorKeymapMode;

mod fast_paste;
mod input;
mod mouse;
mod search;
mod undo;
mod view;
mod word_select_touch;

use search::SearchBox;
use undo::Snapshot;
use word_select_touch::WordSelectTouch;

/// A single open-file editing session, backed by `edtui`. Owns the path
/// it was loaded from (for `save`) and a snapshot of the content as of
/// the last load/save (`is_dirty` used to compare the live buffer to
/// this snapshot on every call, self-correcting if the user undoes
/// their way back to a saved state -- see `dirty`'s own doc comment for
/// why that comparison is now cached instead of redone on every call).
pub struct Editor {
    path: PathBuf,
    state: EditorState,
    event_handler: EditorEventHandler,
    saved_snapshot: Lines,
    /// Cached result of `state.lines != saved_snapshot`, kept in sync
    /// by `input` rather than recomputed by `is_dirty` itself -- a real
    /// report, made worse but not caused by the syntax-highlighting fix
    /// right above this (`has_pathologically_long_line`'s own doc
    /// comment): a file with one enormous line still felt laggy on
    /// every arrow-key press. `is_dirty` is called once per render
    /// frame (`ui/editor_pane.rs`'s "[modified]" title marker), and
    /// `Lines`' derived `PartialEq` can only ever *disprove* equality
    /// early (the moment two rows' lengths or characters differ) --
    /// proving two buffers *are* equal, which is exactly what happens
    /// on every frame while the cursor is merely moving with no actual
    /// edit, requires walking every character of every row. For an
    /// ordinary file this is unnoticeable; for the one enormous line
    /// that prompted this, it meant a full pass over hundreds of
    /// thousands of characters on every single arrow-key redraw.
    /// `input` now only re-runs that comparison for keys that could
    /// plausibly have mutated the buffer (`can_mutate_buffer`) --
    /// pure navigation (`Left`/`Right`/`Up`/`Down`/`Home`/`End`/
    /// `PageUp`/`PageDown`, with any modifiers -- confirmed from
    /// `bindings/mod.rs`'s own table that `Shift` on these only ever
    /// extends a selection and `Ctrl` only ever changes the jump
    /// granularity, neither ever reaches an `Insert`/`Delete`/paste
    /// action) reuses whatever `dirty` already was instead of paying
    /// this cost again. `select_all`/`extend_word_selection` (`Ctrl+A`/
    /// `Ctrl+Shift+Left`/`Right`) bypass `input` entirely
    /// (`editor_keymap::handle_editor_key`) and never touch buffer
    /// content either, so leaving `dirty` untouched on those paths is
    /// correct too, not just unaddressed.
    dirty: bool,
    /// Overrides `SYNTAX_THEME`'s named lookup when the user has a
    /// custom color scheme configured — see `config::load_active_theme`
    /// and `.claude/rules/litastum-theming.md`.
    custom_syntax_theme: Option<SynTheme>,
    /// The file's first line as of `open()` — the input to
    /// `resolve_syntax_highlighter`'s first-line lookup tier (e.g.
    /// `.git/config`'s `^\[core\]`). Captured once at open rather than
    /// re-derived from the live buffer on every `view()` call: matches
    /// what a real first-line grammar detection is meant to see (the
    /// file as opened), and avoids re-flattening the jagged `Lines`
    /// buffer into a `String` every frame just to peek at row 0.
    first_line: String,
    /// What `Ctrl+Shift+Left`/`Right` (word-wise selection) has done to
    /// the *current* selection so far -- see `word_select_touch`
    /// module's own doc comment on `WordSelectTouch` for what each
    /// variant means and `extend_word_selection`'s body for exactly how
    /// it's read and updated.
    word_select_touch: WordSelectTouch,
    /// The column a `Shift+Up`/`Down` selection actually started at,
    /// before `exclude_landing_column_on_fresh_vertical_selection`
    /// trims one edge of it by a column -- `None` whenever no such
    /// selection is open. Has to live here, not inside `EditorState`
    /// itself: the trim mutates `state.cursor.col`/`selection.start.col`
    /// directly (there's no other way to exclude the aligned landing
    /// column from an inclusive-both-ends selection), so once that
    /// happens neither field reliably remembers the pre-trim value any
    /// more -- `close_selection_if_back_on_the_anchors_row` reads this
    /// back to restore it once the excursion closes. See
    /// `bindings::shift_select`'s own doc comment for the full history
    /// of why this needed its own tracked field rather than being
    /// re-derivable from `EditorState` alone.
    vertical_shift_anchor_col: Option<usize>,
    /// The cursor position a `Ctrl+Shift+Left`-built word selection
    /// actually started at, before `trim_anchor_off_a_word_it_never_visited`
    /// trims `selection.start` back by a column -- `None` whenever no
    /// such selection is open. Same shape of problem as
    /// `vertical_shift_anchor_col` above, for the same reason: once the
    /// trim runs, `selection.start` no longer records the real starting
    /// point, so a later `Ctrl+Shift+Right` that retraces this walk back
    /// past its own start (`bindings::word_select::
    /// retreat_forward_through_a_backward_walk`) has nowhere else to
    /// recover the true value from. Reported directly: retracing
    /// "deri" (trimmed from "derived", cursor originally between 'i'
    /// and 'v') back past its own start landed the cursor one column
    /// short, between 'r' and 'i', instead of back at the exact
    /// original position between 'i' and 'v' -- see that function's own
    /// doc comment for the fix and for the VS Code-matching "reflect
    /// forward from there" behavior this also unlocks.
    word_select_true_anchor: Option<Index2>,
    /// Which entry of the search history (`App::search_history`,
    /// threaded in by the caller -- `Editor` itself doesn't own the
    /// list) `Up`/`Down` last recalled into the search box, `None`
    /// while not currently browsing it at all. See `search` module's
    /// own `search_history_up`/`_down` doc comments for the shell-
    /// `Up`-arrow convention this follows, and `search_push_char`/
    /// `_pop_char`/`accept_search_suggestion` for why editing the query
    /// any other way resets this back to `None`.
    search_history_index: Option<usize>,
    /// The open `Ctrl+F` search's own matching state -- `None` while the
    /// box is closed. See `search::SearchSession` for why this replaces
    /// `edtui`'s own search entirely. Created lazily on the first typed
    /// character too, not only by `start_search`, so a search opened
    /// some other way (Vim's own `/`) still starts from where the cursor
    /// actually was.
    search: Option<SearchBox>,
    /// Which key-binding scheme this session currently uses -- see
    /// `EditorKeymapMode`'s own doc comment. Drives both which
    /// `event_handler` was built with (`Editor::open`/`set_keymap_mode`)
    /// and whether `input`'s own post-table correction passes run at
    /// all (`Standard`-only, per `EditorKeymapMode::Vim`'s own doc
    /// comment on why).
    keymap_mode: EditorKeymapMode,
    /// Extra `Highlight`s merged into `state.highlights` on top of
    /// whatever `view()` already computes (word-occurrence/bracket-pair
    /// matching) -- added for `compare::CompareState`, which needs to
    /// paint GitHub-style red/green diff backgrounds over an otherwise
    /// perfectly ordinary, fully editable `Editor`, without duplicating
    /// `view()`'s own rendering logic. Empty and inert for every other
    /// caller (plain `F4` editing never sets this).
    extra_highlights: Vec<Highlight>,
    /// Whether `view()` should resolve and apply syntax highlighting at
    /// all -- `true` (matching every existing caller's own expectation)
    /// unless `disable_syntax_highlighting` was called. Added for
    /// `compare::CompareState`: requested directly, after real use
    /// showed per-token syntax coloring fighting for attention with the
    /// GitHub-style diff backgrounds this view already paints over
    /// changed lines -- plain themed text keeps the diff coloring itself
    /// the one thing drawing the eye. `F4` editing never touches this.
    syntax_highlighting_enabled: bool,
    /// This app's own undo stack for `Standard`-keymap editing, most
    /// recent last -- replaces `edtui`'s own `Undo`/`capture_on_insert`
    /// mechanism entirely for this keymap (see `input`'s own doc
    /// comment for why: `EditorState::capture()`, what `edtui`'s own
    /// `Undo` pops against, is `pub(crate)`, so this app's own fast
    /// paste could never record a checkpoint on it, and once any other
    /// edit happened after a paste, `edtui`'s own undo had no boundary
    /// left to jump back to in one step -- a real reported bug).
    /// `Vim` keeps `edtui`'s own real mechanism untouched, same "no
    /// correction pass runs for Vim" rule every other part of this
    /// keymap already follows -- this field simply stays empty there.
    undo_stack: Vec<Snapshot>,
    /// This keymap's own redo stack, the mirror of `undo_stack` -- `Ctrl+Y`
    /// pops from here and pushes the state it's leaving onto
    /// `undo_stack`, same as any conventional editor's redo. Cleared
    /// whenever a *new* edit is captured (`push_undo_snapshot`) -- an
    /// edit made after undoing invalidates whatever redo history came
    /// before it, same convention every other editor's redo already
    /// follows.
    redo_stack: Vec<Snapshot>,
    /// The whole screen area this editor was last drawn into
    /// (`view()`'s own `area`) -- lets a mouse event be routed to the
    /// editor only when it actually lands on it (`contains_screen_position`),
    /// not to a linked Markdown preview drawn beside it.
    view_area: ratatui::layout::Rect,
}


/// Builds the real `edtui` event handler for `mode` -- `Standard` keeps
/// this project's own non-modal table (`bindings::standard_key_handler`),
/// `Vim` uses `edtui`'s own bundled binding set unmodified
/// (`EditorEventHandler::vim_mode`), per `EditorKeymapMode::Vim`'s own
/// doc comment on why this project doesn't try to layer its own
/// correction passes on top of it.
fn event_handler_for(mode: EditorKeymapMode) -> EditorEventHandler {
    match mode {
        EditorKeymapMode::Standard => EditorEventHandler::new(standard_key_handler()),
        EditorKeymapMode::Vim => EditorEventHandler::vim_mode(),
    }
}

/// Each keymap's own natural starting `EditorMode` -- `Standard` always
/// starts typing immediately (`Insert`, matching every non-modal editor
/// this app is modeled on), `Vim` starts in `Normal`, matching real
/// Vim's own convention (and `edtui`'s own `vim_mode()` binding table,
/// which expects to begin there). The two `Insert` values these keymaps
/// *do* share aren't the same concept -- this project's own `Insert` is
/// "the only mode `Standard` ever uses," Vim's `Insert` is one of
/// several modes reached and left via its own bindings (`i`, `Esc`,
/// ...) -- so switching keymaps resets to each one's own starting point
/// rather than trying to carry a mode across.
fn starting_mode(mode: EditorKeymapMode) -> EditorMode {
    match mode {
        EditorKeymapMode::Standard => EditorMode::Insert,
        EditorKeymapMode::Vim => EditorMode::Normal,
    }
}


impl Editor {
    /// Loads `path`'s contents into a new editing session. Fails if the
    /// file can't be read as UTF-8 text (binary files aren't supported
    /// yet — see `TODO/editor.md`). `custom_syntax_theme` is `None` for the
    /// built-in named syntax theme, or a scheme-derived theme when the
    /// user has a custom color scheme configured. `keymap_mode` is
    /// normally `App::editor_keymap_mode` (the session-wide default,
    /// itself loaded from `config.json`) -- see `EditorKeymapMode`'s own
    /// doc comment.
    pub fn open(path: PathBuf, custom_syntax_theme: Option<SynTheme>, keymap_mode: EditorKeymapMode) -> io::Result<Self> {
        let contents = fs::read_to_string(&path)?;
        let lines = Lines::from(contents.as_str());
        let first_line = contents.lines().next().unwrap_or("").to_string();

        let mut state = EditorState::new(lines.clone());
        state.mode = starting_mode(keymap_mode);
        state.set_clipboard(OsClipboardBridge);

        Ok(Self {
            path,
            state,
            event_handler: event_handler_for(keymap_mode),
            saved_snapshot: lines,
            dirty: false,
            custom_syntax_theme,
            first_line,
            word_select_touch: WordSelectTouch::Untouched,
            vertical_shift_anchor_col: None,
            word_select_true_anchor: None,
            search_history_index: None,
            search: None,
            keymap_mode,
            extra_highlights: Vec::new(),
            syntax_highlighting_enabled: true,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            view_area: ratatui::layout::Rect::default(),
        })
    }


    /// This session's currently active key-binding scheme -- read by
    /// `keymap_menu.rs` to open its own picker already highlighting the
    /// right entry.
    pub fn keymap_mode(&self) -> EditorKeymapMode {
        self.keymap_mode
    }

    /// Whether this editor is in ordinary `Standard`-keymap typing mode
    /// right now -- `event_loop::keys::drain_pending_editor_typing`'s own gate
    /// for safely batch-fast-pathing a burst of already-queued plain
    /// character keys (see that function's own doc comment for the real
    /// reported bug this exists to fix). `Vim`'s `Normal` mode treats a
    /// bare letter as a command, not text, and `Standard`'s own
    /// `Visual` mode has its own selection-aware typing behavior
    /// (`is_selection_consuming_key`) -- batching either of those as if
    /// every queued character were literal paste content would silently
    /// run/misinterpret whatever was actually queued instead of typing
    /// it.
    pub(crate) fn is_plain_standard_typing(&self) -> bool {
        self.keymap_mode == EditorKeymapMode::Standard && self.state.mode == EditorMode::Insert
    }


    /// Live-switches this already-open session's own keymap --
    /// `keymap_menu.rs`'s own `Enter` handling, so a mode change applies
    /// immediately without closing and reopening the file. Rebuilds
    /// `event_handler` from scratch (there's no incremental way to swap
    /// `edtui`'s own binding table) and resets `state.mode` to each
    /// mode's own natural starting point (`starting_mode`'s own doc
    /// comment) -- deliberately *not* preserved across the switch, since
    /// `Standard`'s `Insert` and `Vim`'s `Normal` aren't the same concept
    /// even though they share a variant name in `edtui`'s own
    /// `EditorMode` enum, and leaving whichever one was active can leave
    /// the editor in a state its own keymap doesn't expect (e.g. `Vim`
    /// while still marked `Insert`, `Standard`'s own correction passes
    /// running against a mode they were never tuned for). Any active
    /// selection is intentionally dropped for the same reason -- neither
    /// keymap's own selection semantics carry over meaningfully to the
    /// other.
    pub fn set_keymap_mode(&mut self, mode: EditorKeymapMode) {
        self.keymap_mode = mode;
        self.event_handler = event_handler_for(mode);
        self.state.mode = starting_mode(mode);
        self.state.selection = None;
    }


    /// `Ctrl+A` -- selects the entire buffer. Built from the same
    /// primitive `edtui` motions everything else in this file already
    /// uses rather than constructing a `Selection` by hand (its fields
    /// are `pub(crate)`, unreachable from here anyway -- see
    /// [[litastum-stack]]'s word-selection history for the general
    /// pattern this follows): jump to the very first cell, open a
    /// selection there (`SwitchMode(Visual)` anchors on the *current*
    /// cursor, so the jump has to happen first), then jump to the very
    /// last cell -- `MoveToLastRow`/`MoveToEndOfLine` each call
    /// `edtui`'s own `set_selection_with_lines` while in `Visual` mode,
    /// exactly like every other selection-extending motion here, so the
    /// selection ends up spanning the whole buffer with no bespoke
    /// selection-building logic at all.
    pub fn select_all(&mut self) {
        MoveToFirstRow()
            .chain(MoveToStartOfLine())
            .chain(SwitchMode(EditorMode::Visual))
            .chain(MoveToLastRow())
            .chain(MoveToEndOfLine())
            .execute(&mut self.state);
    }


    /// Whether there's an active text selection (used by the caller to
    /// decide whether `Esc` should cancel the selection or close the
    /// editor).
    pub fn has_selection(&self) -> bool {
        self.state.selection.is_some()
    }

    /// The cursor's raw buffer position (row/column into `state.lines`,
    /// not a screen position -- see `cursor_screen_position` for that).
    /// Used to be `#[cfg(test)]`-only (a plain accessor with no non-test
    /// caller flagged `dead_code`) until `compare::CompareState` needed
    /// it for real, to save/restore a pane's true cursor position across
    /// focus switches -- see `set_cursor`/`set_viewport_top_row` below.
    pub fn cursor(&self) -> Index2 {
        self.state.cursor
    }

    /// Moves the cursor directly, with no motion/selection semantics --
    /// used by `compare::CompareState` to restore a pane's real cursor
    /// position when it regains focus, after `set_viewport_top_row`
    /// below temporarily repurposed `state.cursor.row` for viewport
    /// syncing while this pane was the *other* (unfocused) one.
    pub fn set_cursor(&mut self, pos: Index2) {
        self.state.cursor = pos;
    }

    /// Forces this editor's viewport to start at `row`, keeping it there
    /// through the next render -- for the currently *unfocused* Compare
    /// pane, so its visible rows stay diff-aligned with whatever the
    /// focused pane is showing (`compare::diff::map_real_row`).
    ///
    /// Also overwrites `state.cursor.row`, not just the viewport offset:
    /// `edtui`'s own render pass recomputes the viewport from the cursor
    /// on every frame to keep it visible (`EditorState::set_viewport_offset`'s
    /// own doc comment; the exact mechanism a real report already traced
    /// through once for the read-only phase-1 Compare view, see
    /// `ui/compare.rs`'s own history) -- without this, a cursor left
    /// behind at its last real edit position would just snap the
    /// viewport straight back there on the very next render, undoing
    /// this call entirely. Safe to do here specifically because this is
    /// only ever called on the *unfocused* pane, which never receives
    /// key input and so never needs `state.cursor` to mean anything else
    /// while it's called -- `CompareState` caches the real cursor
    /// position before overriding it, and `set_cursor` restores it the
    /// moment focus returns.
    pub fn set_viewport_top_row(&mut self, row: usize) {
        let row = row.min(self.state.lines.len().saturating_sub(1));
        let (offset_x, _) = self.state.viewport_offset();
        self.state.cursor.row = row;
        self.state.set_viewport_offset(offset_x, row);
    }

    /// The buffer's current content as plain text -- used by
    /// `compare::CompareState` to recompute the live diff between both
    /// panes on every frame, straight from what's actually being edited
    /// rather than a stale on-open snapshot.
    pub fn text(&self) -> String {
        self.state.lines.to_string()
    }

    /// Extra `Highlight`s merged into this editor's own
    /// word-occurrence/bracket-pair highlights on the next `view()` call
    /// -- see `extra_highlights`'s own doc comment on the struct.
    pub fn set_extra_highlights(&mut self, highlights: Vec<Highlight>) {
        self.extra_highlights = highlights;
    }

    /// Turns off syntax highlighting for this session -- see
    /// `syntax_highlighting_enabled`'s own doc comment on the struct.
    /// One-way by design (no `enable_...` counterpart): every current
    /// caller wants this either always on (`F4` editing) or always off
    /// (`compare::CompareState`) for the lifetime of the session, never
    /// toggled mid-way.
    pub fn disable_syntax_highlighting(&mut self) {
        self.syntax_highlighting_enabled = false;
    }

    /// The editor's current `edtui` mode (`Insert`/`Normal`/`Visual`/
    /// `Search`) -- same sibling-module accessor shape as `cursor`
    /// right above, for the same reason.
    #[cfg(test)]
    pub fn mode(&self) -> EditorMode {
        self.state.mode
    }

    /// The cursor's current row (0-indexed) into the buffer -- used by
    /// `explorer::markdown_preview::state::MarkdownPreviewState::sync_to_editor_cursor`
    /// to keep a linked embedded preview (`App::markdown_edit_preview`)
    /// scrolled to roughly the same source line as whatever's being
    /// edited, and to highlight it there. Requested directly: the
    /// preview's own scrolling should track the editor's cursor,
    /// highlighting the line currently being edited.
    pub fn cursor_row(&self) -> usize {
        self.state.cursor.row
    }

    /// The buffer row currently scrolled to the *top* of the editor's
    /// own visible area -- `edtui`'s own live viewport offset
    /// (`EditorState::viewport_offset`), updated as part of its normal
    /// rendering. Combined with `cursor_row` by `ui::draw` to work out
    /// how far down its own visible page the cursor currently sits, so
    /// a linked embedded preview (`App::markdown_edit_preview`) can
    /// scroll to roughly the same *relative* position on its own page
    /// rather than always snapping the matched line to its own top --
    /// requested directly, after a first, top-aligned version put the
    /// highlighted line at a visibly different screen row than the
    /// cursor whenever editing wasn't already at the very top of the
    /// editor: the two should stay roughly level, so editing partway
    /// down the page keeps the preview's own highlight at about the
    /// same height.
    pub fn viewport_top_row(&self) -> usize {
        self.state.viewport_offset().1
    }


    /// Writes the current buffer back to the file it was opened from.
    pub fn save(&mut self) -> io::Result<()> {
        let contents = String::from(self.state.lines.clone());
        debug!(path = %self.path.display(), bytes = contents.len(), "editor save: writing");
        match fs::write(&self.path, &contents) {
            Ok(()) => {
                self.saved_snapshot = self.state.lines.clone();
                self.dirty = false;
                debug!(path = %self.path.display(), "editor save: ok");
                Ok(())
            }
            Err(err) => {
                warn!(path = %self.path.display(), %err, "editor save: failed");
                Err(err)
            }
        }
    }


    /// Everything that has to follow a change to the buffer's contents:
    /// the cached `dirty` flag (see its own doc comment) and, if the
    /// `Ctrl+F` box is open, its matches -- the box can stay open while
    /// the text is edited (`is_searching`'s own doc comment). One place
    /// for every mutation path (typing, paste, undo/redo) so neither can
    /// be forgotten on one of them.
    fn buffer_changed(&mut self) {
        self.dirty = self.state.lines != self.saved_snapshot;
        self.refresh_search_matches();
    }


    /// Whether the buffer differs from the last loaded/saved snapshot --
    /// an O(1) read of the cached `dirty` field (see its own doc
    /// comment on `Editor` for why this used to recompare the whole
    /// buffer on every call, and why that stopped being cheap enough).
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
}


#[cfg(test)]
mod tests;
