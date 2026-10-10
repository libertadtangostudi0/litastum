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

mod block_move;
mod changes;
mod click;
mod fast_paste;
mod input;
mod mouse;
mod search;
mod selection_delete;
mod undo;
mod view;

pub(crate) use view::fitted_title;
mod word_select_touch;

use search::SearchBox;
use undo::Snapshot;
use word_select_touch::WordSelectTouch;

/// A single open-file editing session, backed by `edtui`. Owns the path
/// it was loaded from (for `save`) and the content as of the last
/// load/save (`saved`, for `is_dirty`).
pub struct Editor {
    path: PathBuf,
    state: EditorState,
    event_handler: EditorEventHandler,
    /// `None` until the first edit since load or save (`remember_saved_rows`).
    saved: Option<changes::SavedRows>,
    /// Where `state.lines` differs from `saved` (`changes.rs`),
    /// kept up to date per edit -- proving two buffers equal walks every
    /// character, and `is_dirty` runs every frame, which made arrow keys
    /// lag on a file with one enormous line. History:
    /// docs/history/editor-performance.md.
    differing: changes::Differing,
    /// Whether some row is too long to highlight
    /// (`has_pathologically_long_line`), kept per edit rather than
    /// scanned every frame.
    has_long_line: bool,
    /// Changes with every change to the text (`changes::next_revision`).
    revision: u64,
    /// Overrides `SYNTAX_THEME` when a custom `editor_theme` is
    /// configured (`.claude/rules/litastum-theming.md`).
    custom_syntax_theme: Option<SynTheme>,
    /// The file's first line as of `open()`, for the first-line grammar
    /// lookup (e.g. `.git/config`'s `^\[core\]`). Captured once rather
    /// than flattened from the live buffer every frame.
    first_line: String,
    /// What `Ctrl+Shift+Left`/`Right` has done to the current selection
    /// so far (`WordSelectTouch`). History: docs/history/word-select.md.
    word_select_touch: WordSelectTouch,
    /// Where a `Shift+Up`/`Down` selection started, before
    /// `exclude_landing_column_on_fresh_vertical_selection` trimmed it, and
    /// whether its end sits on a line break; `None` when no such selection
    /// is open. Lives here because the trim overwrites the `EditorState`
    /// fields that could have held it. History: docs/history/shift-select.md.
    vertical_shift_anchor: Option<super::bindings::VerticalAnchor>,
    /// Where a `Ctrl+Shift+Left`-built selection started, before
    /// `trim_anchor_off_a_word_it_never_visited` trimmed `selection.start`;
    /// `None` when no such selection is open. Retracing the walk back past
    /// its start restores this exact position. History:
    /// docs/history/word-select.md (16).
    word_select_true_anchor: Option<Index2>,
    /// Which `App::search_history` entry `Up`/`Down` last recalled into
    /// the search box; `None` when not browsing it. Any other edit to the
    /// query resets it.
    search_history_index: Option<usize>,
    /// The open `Ctrl+F` search (`None` while closed) -- our own
    /// `SearchSession`, not `edtui`'s. Also created lazily on the first
    /// typed character, so a search opened another way (Vim's `/`) starts
    /// from the real cursor.
    search: Option<SearchBox>,
    /// This session's key-binding scheme: decides `event_handler` and
    /// whether `input`'s correction passes run (`Standard` only).
    keymap_mode: EditorKeymapMode,
    /// Extra highlights merged over `view()`'s own -- Compare's and the
    /// conflict resolver's row backgrounds on an otherwise ordinary
    /// `Editor`. Empty elsewhere. One row each, sorted by row, so a frame
    /// hands `edtui` only the visible ones (`highlights_on`).
    extra_highlights: Vec<Highlight>,
    /// Off only for Compare (`disable_syntax_highlighting`): token colors
    /// competed with the diff backgrounds.
    syntax_highlighting_enabled: bool,
    /// Undo stack for the `Standard` keymap, most recent last -- replaces
    /// `edtui`'s own undo there (`.claude/rules/litastum-editor-undo.md`).
    /// Stays empty for `Vim`.
    undo_stack: Vec<Snapshot>,
    /// Redo stack, the mirror of `undo_stack`; cleared by a new edit.
    redo_stack: Vec<Snapshot>,
    /// The row the key being handled edits, when it edits only that one
    /// (`input::edits_only_the_cursor_row`): set before `edtui`'s
    /// dispatch, taken after it.
    row_edit: Option<usize>,
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
        EditorKeymapMode::Standard => EditorEventHandler::new(standard_key_handler(false)),
        EditorKeymapMode::Vim => EditorEventHandler::vim_mode(),
    }
}

/// Each keymap's starting mode: `Standard` types immediately (`Insert`),
/// `Vim` starts in `Normal`. The two keymaps' `Insert` aren't the same
/// concept, so switching resets to the start instead of carrying the mode
/// over.
fn starting_mode(mode: EditorKeymapMode) -> EditorMode {
    match mode {
        EditorKeymapMode::Standard => EditorMode::Insert,
        EditorKeymapMode::Vim => EditorMode::Normal,
    }
}


impl Editor {
    /// Opens `path` for editing; fails if it isn't UTF-8 text (`TODO/editor.md`).
    /// `custom_syntax_theme` is `None` for the built-in theme. `keymap_mode`
    /// is normally `App::settings.editor_keymap_mode`.
    pub fn open(path: PathBuf, custom_syntax_theme: Option<SynTheme>, keymap_mode: EditorKeymapMode) -> io::Result<Self> {
        let contents = fs::read_to_string(&path)?;
        let lines = Lines::from(contents.as_str());
        let first_line = contents.lines().next().unwrap_or("").to_string();

        let has_long_line = super::word_highlight::has_pathologically_long_line(&lines);
        let mut state = EditorState::new(lines);
        state.mode = starting_mode(keymap_mode);
        state.set_clipboard(OsClipboardBridge);

        Ok(Self {
            path,
            state,
            event_handler: event_handler_for(keymap_mode),
            saved: None,
            differing: changes::Differing::Nowhere,
            has_long_line,
            revision: changes::next_revision(),
            custom_syntax_theme,
            first_line,
            word_select_touch: WordSelectTouch::Untouched,
            vertical_shift_anchor: None,
            word_select_true_anchor: None,
            search_history_index: None,
            search: None,
            keymap_mode,
            extra_highlights: Vec::new(),
            syntax_highlighting_enabled: true,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            row_edit: None,
            view_area: ratatui::layout::Rect::default(),
        })
    }


    /// Opens `path` as a new session with this one's syntax theme, keymap
    /// and syntax-highlighting switch -- Compare's path field loading
    /// another file into a pane.
    pub fn reopen(&self, path: PathBuf) -> io::Result<Self> {
        let mut editor = Self::open(path, self.custom_syntax_theme.clone(), self.keymap_mode)?;
        editor.syntax_highlighting_enabled = self.syntax_highlighting_enabled;
        Ok(editor)
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// This session's currently active key-binding scheme -- read by
    /// `keymap_menu.rs` to open its own picker already highlighting the
    /// right entry.
    pub fn keymap_mode(&self) -> EditorKeymapMode {
        self.keymap_mode
    }

    /// Whether a plain character typed now is just text: `Standard` in
    /// `Insert`. Gates batching queued keys into one insert
    /// (`event_loop::keys::drain_pending_editor_typing`) -- in Vim's
    /// `Normal` a letter is a command, and `Visual` has its own
    /// selection-aware typing.
    pub(crate) fn is_plain_standard_typing(&self) -> bool {
        self.keymap_mode == EditorKeymapMode::Standard && self.state.mode == EditorMode::Insert
    }


    /// Switches this open session's keymap live (the keymap menu's
    /// `Enter`). Rebuilds `event_handler` and resets to `starting_mode`,
    /// dropping any selection -- neither mode nor selection semantics
    /// carry over between the keymaps.
    pub fn set_keymap_mode(&mut self, mode: EditorKeymapMode) {
        self.keymap_mode = mode;
        self.event_handler = event_handler_for(mode);
        self.state.mode = starting_mode(mode);
        self.state.selection = None;
    }


    /// `Ctrl+A`. `Selection` can't be constructed from outside `edtui`,
    /// so this jumps to the first cell, opens a selection there, and
    /// jumps to the last -- the motions extend a `Visual` selection
    /// themselves.
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

    /// The cursor's buffer position (not a screen position -- see
    /// `cursor_screen_position`). Compare saves/restores it across focus
    /// switches.
    pub fn cursor(&self) -> Index2 {
        self.state.cursor
    }

    /// Moves the cursor directly, no motion/selection semantics -- Compare
    /// restores a pane's real cursor with it after `set_viewport_top_row`.
    pub fn set_cursor(&mut self, pos: Index2) {
        self.state.cursor = pos;
    }

    /// Moves the cursor to `pos` for a jump (a `Ctrl+F` match, a Compare
    /// hunk) and scrolls its row to the middle of the view, VS Code-style
    /// -- left to `edtui`, a jump only scrolled into view, onto the last
    /// row. Stops at the first and last lines rather than showing empty
    /// rows. History: docs/history/editor-rendering.md.
    pub fn jump_cursor_to(&mut self, pos: Index2) {
        self.state.cursor = pos;
        if let Some(top) = self.centered_top_row(pos.row) {
            let (offset_x, _) = self.state.viewport_offset();
            self.state.set_viewport_offset(offset_x, top);
        }
    }

    /// Forces this editor's viewport to start at `row`, keeping it there
    /// through the next render -- for the currently *unfocused* Compare
    /// pane, so its visible rows stay diff-aligned with whatever the
    /// focused pane is showing (`compare::diff::map_real_row`).
    ///
    /// Also overwrites `state.cursor.row`: `edtui` re-derives the viewport
    /// from the cursor on every render, so the offset alone would snap
    /// back. Safe only because it's called on the unfocused pane, whose
    /// real cursor `CompareState` saves first and restores with
    /// `set_cursor`.
    pub fn set_viewport_top_row(&mut self, row: usize) {
        let row = row.min(self.state.lines.len().saturating_sub(1));
        let (offset_x, _) = self.state.viewport_offset();
        self.state.cursor.row = row;
        self.state.set_viewport_offset(offset_x, row);
    }

    /// A number for the text's current content: different after any
    /// change, and never shared with another editor. Compare and the
    /// conflict resolver redo their diffs only when it moves.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// How many rows the buffer has.
    pub fn line_count(&self) -> usize {
        self.state.lines.len()
    }

    /// `row`'s text (empty past the end).
    pub fn line_text(&self, row: usize) -> String {
        self.state.lines.get(edtui::RowIndex::new(row)).map_or_else(String::new, |chars| chars.iter().collect())
    }

    /// How many characters `row` has (0 past the end).
    pub fn line_len(&self, row: usize) -> usize {
        self.state.lines.len_col(row).unwrap_or(0)
    }

    /// The buffer's current content -- Compare recomputes its live diff
    /// from this.
    pub fn text(&self) -> String {
        changes::lines_to_string(&self.state.lines)
    }

    /// See the `extra_highlights` field: one row each.
    pub fn set_extra_highlights(&mut self, mut highlights: Vec<Highlight>) {
        highlights.sort_by_key(|highlight| highlight.start.row);
        self.extra_highlights = highlights;
    }



    /// Turns syntax highlighting off for the session. One-way: callers
    /// want it always on (F4) or always off (Compare).
    pub fn disable_syntax_highlighting(&mut self) {
        self.syntax_highlighting_enabled = false;
    }

    /// The editor's current `edtui` mode.
    #[cfg(test)]
    pub fn mode(&self) -> EditorMode {
        self.state.mode
    }

    /// The cursor's row -- the linked Markdown preview scrolls to and
    /// highlights the same source line
    /// (`MarkdownPreviewState::sync_to_editor_cursor`).
    pub fn cursor_row(&self) -> usize {
        self.state.cursor.row
    }

    /// The buffer row at the top of the visible area. With `cursor_row`
    /// it gives the cursor's height on the page, so the linked preview
    /// keeps its highlighted line level with the cursor rather than
    /// snapping it to the top.
    pub fn viewport_top_row(&self) -> usize {
        self.state.viewport_offset().1
    }


    /// Writes the current buffer back to the file it was opened from.
    pub fn save(&mut self) -> io::Result<()> {
        let contents = changes::lines_to_string(&self.state.lines);
        debug!(path = %self.path.display(), bytes = contents.len(), "editor save: writing");
        match fs::write(&self.path, &contents) {
            Ok(()) => {
                self.saved = None;
                self.differing = changes::Differing::Nowhere;
                debug!(path = %self.path.display(), "editor save: ok");
                Ok(())
            }
            Err(err) => {
                warn!(path = %self.path.display(), %err, "editor save: failed");
                Err(err)
            }
        }
    }


    /// `Shift+F2`: writes the buffer to `path`, which becomes this
    /// editor's file (and grammar). The old path stays on failure.
    pub fn save_as(&mut self, path: PathBuf) -> io::Result<()> {
        let previous = std::mem::replace(&mut self.path, path);
        self.save().inspect_err(|_| self.path = previous)
    }


    /// Whether the buffer differs from the last loaded/saved content --
    /// an O(1) read of `differing`.
    pub fn is_dirty(&self) -> bool {
        self.differing != changes::Differing::Nowhere
    }
}



/// The highlights in `sorted` (by row, one row each) on `rows`. `edtui`
/// checks every highlight it's given while drawing, so passing all of a
/// large diff's rows made a frame several times slower, the more so the
/// more lines a small font put on screen.
fn highlights_on(sorted: &[Highlight], rows: std::ops::Range<usize>) -> &[Highlight] {
    let first = sorted.partition_point(|highlight| highlight.start.row < rows.start);
    let end = sorted.partition_point(|highlight| highlight.start.row < rows.end);
    &sorted[first..end.max(first)]
}


#[cfg(test)]
mod tests;
