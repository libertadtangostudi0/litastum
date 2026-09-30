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
/// it was loaded from (for `save`) and the content as of the last
/// load/save (`saved_snapshot`, for `is_dirty`).
pub struct Editor {
    path: PathBuf,
    state: EditorState,
    event_handler: EditorEventHandler,
    saved_snapshot: Lines,
    /// Cached `state.lines != saved_snapshot`. Recomputed only after keys
    /// that can mutate the buffer (`can_mutate_buffer`) and in
    /// `buffer_changed` -- proving two buffers equal walks every
    /// character, and `is_dirty` runs every frame, which made arrow keys
    /// lag on a file with one enormous line. History:
    /// docs/history/editor-performance.md.
    dirty: bool,
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
    /// The column a `Shift+Up`/`Down` selection started at, before
    /// `exclude_landing_column_on_fresh_vertical_selection` trimmed it;
    /// `None` when no such selection is open. Lives here because the trim
    /// overwrites both `EditorState` fields that could have held it;
    /// `close_selection_if_back_on_the_anchors_row` restores it. History:
    /// docs/history/shift-select.md.
    vertical_shift_anchor_col: Option<usize>,
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
    /// Extra highlights merged over `view()`'s own -- Compare's diff
    /// backgrounds on an otherwise ordinary `Editor`. Empty elsewhere.
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

    /// The buffer's current content -- Compare recomputes its live diff
    /// from this.
    pub fn text(&self) -> String {
        self.state.lines.to_string()
    }

    /// See the `extra_highlights` field.
    pub fn set_extra_highlights(&mut self, highlights: Vec<Highlight>) {
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


    /// Whether the buffer differs from the last loaded/saved content --
    /// an O(1) read of the cached `dirty`.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
}


#[cfg(test)]
mod tests;
