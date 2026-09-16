use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};
use edtui::actions::motion::{MoveToFirstRow, MoveToLastRow};
use edtui::actions::{Chainable, Execute, MoveToEndOfLine, MoveToStartOfLine, SwitchMode};
use edtui::syntect::highlighting::Theme as SynTheme;
use edtui::{EditorEventHandler, EditorMode, EditorState, EditorTheme, EditorView, Index2, LineNumbers, Lines};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Block;
use tracing::{debug, warn};

use crate::theming::Theme;

use super::bindings::{
    anchor_fresh_shift_selection, close_selection_if_back_on_the_anchors_row, exclude_landing_column_on_fresh_vertical_selection,
    is_selection_consuming_key, standard_key_handler, wrap_line_boundary_arrow_movement,
};
use super::bracket_match::{bracket_match_highlights, cursor_is_on_a_matched_bracket, matched_bracket_row_span};
use super::clipboard::OsClipboardBridge;
use super::keymap_mode::EditorKeymapMode;
use super::syntax::resolve_syntax_highlighter;
use super::word_highlight::{has_pathologically_long_line, word_occurrence_highlights};

mod search;
mod word_select_touch;

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
    /// Which key-binding scheme this session currently uses -- see
    /// `EditorKeymapMode`'s own doc comment. Drives both which
    /// `event_handler` was built with (`Editor::open`/`set_keymap_mode`)
    /// and whether `input`'s own post-table correction passes run at
    /// all (`Standard`-only, per `EditorKeymapMode::Vim`'s own doc
    /// comment on why).
    keymap_mode: EditorKeymapMode,
}


/// Whether `code` could plausibly have changed `state.lines` --
/// deliberately conservative (defaults to `true`, "might have
/// mutated") for anything not on this short, confirmed-safe list.
/// Checked regardless of modifiers: per `bindings/mod.rs`'s own key
/// table, `Shift` on any of these only ever extends a selection and
/// `Ctrl` only ever changes the jump granularity (word-wise, half-page)
/// -- neither ever reaches an `Insert`/`Delete`/paste action on this
/// list's own keys. See `Editor::dirty`'s own doc comment for why this
/// distinction exists at all.
fn can_mutate_buffer(code: KeyCode) -> bool {
    !matches!(
        code,
        KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down | KeyCode::Home | KeyCode::End | KeyCode::PageUp | KeyCode::PageDown
    )
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
            keymap_mode,
        })
    }


    /// This session's currently active key-binding scheme -- read by
    /// `keymap_menu.rs` to open its own picker already highlighting the
    /// right entry.
    pub fn keymap_mode(&self) -> EditorKeymapMode {
        self.keymap_mode
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


    /// Feeds one key event to the editor. Standard (non-modal) editing
    /// bindings — see `standard_key_handler` — everything not bound
    /// there that's a plain character still inserts, since the editor
    /// stays in `EditorMode::Insert` outside an active selection.
    ///
    /// Post-table correction passes run in sequence, all only ever
    /// *adding* behavior the table's own declarative chaining couldn't
    /// express on its own (see each one's own doc comment for why):
    ///
    /// 0. `is_selection_consuming_key` -- only for the five visual-mode
    ///    bindings (`Backspace`/`Delete`/`Ctrl+C`/`Ctrl+X`/`Ctrl+V`) that
    ///    consume an active selection. Resets `state.selection`/`state.mode`
    ///    back to plain typing by direct field assignment rather than
    ///    `edtui`'s own `SwitchMode(Insert)` -- see `is_selection_consuming_key`'s
    ///    own doc comment for the real bug this avoids (a redundant undo
    ///    checkpoint that made `Ctrl+Z` need two presses instead of one).
    /// 1. `anchor_fresh_shift_selection` -- only when this key just
    ///    transitioned a fresh selection into `Visual` mode (a plain,
    ///    non-word-select `Shift+Left`/`Right` with nothing already
    ///    selected -- `Shift+Up`/`Down` don't need this, see that
    ///    function's own doc comment for why). If the cell it anchored
    ///    on holds a real character, this is already exactly the wanted
    ///    one-character selection and nothing more happens; otherwise it
    ///    falls back to performing the actual move.
    /// 2. `wrap_line_boundary_arrow_movement` -- adds a row change when
    ///    the table's own handling of a plain/shifted `Left`/`Right`
    ///    turned out to be a no-op at a line boundary. Skipped entirely
    ///    when step 1 just deliberately left the cursor unmoved on a
    ///    real character -- that zero movement is a correct, intentional
    ///    stop (a fresh selection is exactly one character), not a
    ///    signal that a plain arrow press hit a wall, and treating it as
    ///    one would wrongly wrap an ordinary mid-line `Shift+Right` down
    ///    into the next line.
    /// 3. `exclude_landing_column_on_fresh_vertical_selection` -- only
    ///    when this key just opened a fresh `Shift+Up`/`Down` selection.
    ///    Trims the aligned landing column out of it (see that
    ///    function's own doc comment for the real report), and records
    ///    the pre-trim column into `vertical_shift_anchor_col` first, so
    ///    step 4 below can restore it later.
    /// 4. `close_selection_if_back_on_the_anchors_row` -- runs
    ///    unconditionally for every `Shift+Up`/`Down` press, fresh or
    ///    continuing. Closes the selection entirely once the cursor
    ///    lands back on the exact row it started a vertical excursion
    ///    from, since `MoveUp`/`MoveDown` never touch the column, so
    ///    that always means landing back on the anchor exactly --
    ///    without this, a `Shift+Down`+`Shift+Up` round trip (or the
    ///    reverse) would leave a phantom one-character selection instead
    ///    of returning to nothing, since `edtui`'s inclusive model can't
    ///    represent a zero-width selection on its own. Also restores
    ///    `state.cursor.col` from `vertical_shift_anchor_col` (step 3's
    ///    tracked value) at the same time -- without that, step 3's own
    ///    trim would leave the cursor permanently one column short of
    ///    where the excursion actually started.
    ///
    /// Every other key (and every already-working press these don't
    /// apply to) passes through completely unaffected.
    pub fn input(&mut self, key: KeyEvent) {
        let cursor_before = self.state.cursor;
        let mode_before = self.state.mode;
        self.event_handler.on_key_event(key, &mut self.state);

        // Every correction pass below is specifically tuned against
        // `Standard`'s own declarative table (`bindings::standard_key_handler`)
        // -- see `EditorKeymapMode::Vim`'s own doc comment for why none
        // of it runs against `edtui`'s own `vim_mode()` binding table
        // instead: Vim's modal, multi-key sequences were never
        // considered when these were written, and there's no reason to
        // assume they'd interact safely.
        if self.keymap_mode == EditorKeymapMode::Standard {
            if mode_before == EditorMode::Visual && is_selection_consuming_key(&key) {
                self.state.selection = None;
                self.state.mode = EditorMode::Insert;
            }

            let freshly_entered_visual = mode_before != EditorMode::Visual && self.state.mode == EditorMode::Visual;
            let anchored_on_a_real_character =
                freshly_entered_visual && anchor_fresh_shift_selection(&mut self.state, key.code, cursor_before);

            if !anchored_on_a_real_character {
                wrap_line_boundary_arrow_movement(&mut self.state, key.code, key.modifiers, cursor_before);
            }

            if freshly_entered_visual && matches!(key.code, KeyCode::Up | KeyCode::Down) {
                self.vertical_shift_anchor_col = Some(cursor_before.col);
                exclude_landing_column_on_fresh_vertical_selection(&mut self.state, key.code);
            }

            close_selection_if_back_on_the_anchors_row(&mut self.state, key.code, &mut self.vertical_shift_anchor_col);
        }

        if can_mutate_buffer(key.code) {
            self.dirty = self.state.lines != self.saved_snapshot;
        }
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
    /// `editor_keymap.rs`'s own tests are a sibling module of
    /// `editor::editor`, not a descendant, so they can't reach the
    /// private `state` field directly the way `editor::tests` can --
    /// this is the accessor those tests use instead. `#[cfg(test)]`
    /// rather than a plain `pub fn`: this binary has no external
    /// consumers, so with no non-test call site, a normal `cargo build`
    /// (which doesn't see `#[cfg(test)]` code at all, tests included)
    /// flagged it `dead_code` -- gating it the same way removes the
    /// warning honestly instead of silencing it with `#[allow(dead_code)]`
    /// on a method that's genuinely only ever called from tests.
    #[cfg(test)]
    pub fn cursor(&self) -> Index2 {
        self.state.cursor
    }

    /// The cursor's current row (0-indexed) into the buffer -- used by
    /// `explorer::markdown_preview::state::MarkdownPreviewState::sync_to_editor_cursor`
    /// to keep a linked embedded preview (`App::markdown_edit_preview`)
    /// scrolled to roughly the same source line as whatever's being
    /// edited, and to highlight it there. Requested directly
    /// ("прокрутку текста надо сделать одновременной... выделить
    /// строку на превью, которая редактируется в редакторе").
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
    /// editor ("можно... держать примерно на одном уровне... если
    /// редактирование в середине страницы, то и превью в том же
    /// месте").
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


    /// Whether the buffer differs from the last loaded/saved snapshot --
    /// an O(1) read of the cached `dirty` field (see its own doc
    /// comment on `Editor` for why this used to recompare the whole
    /// buffer on every call, and why that stopped being cheap enough).
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }


    /// Builds this frame's renderable view: the editor content plus
    /// syntax highlighting (best-effort — silently skipped if nothing
    /// recognizes this file, by name or by first line) and our theme.
    ///
    /// Takes `&mut self`, unlike a typical read-only render helper:
    /// `EditorView` tracks scroll position as part of rendering, so it
    /// needs write access to `EditorState` even just to draw. `area` is
    /// the exact `Rect` the caller is about to render into -- needed to
    /// work out whether a matched bracket pair spanning multiple rows
    /// can actually both fit on screen at once (see the viewport-nudge
    /// block below); `Editor` has no other way to know the current
    /// render size ahead of the `frame.render_widget` call that
    /// actually consumes the returned `EditorView`.
    pub fn view(&mut self, theme: &Theme, area: Rect) -> EditorView<'_, '_> {
        let custom_syntax_theme = &self.custom_syntax_theme;

        // Try the full file name first, then just the extension —
        // `syntect`'s own convenience lookup (`SyntaxSet::find_syntax_for_file`)
        // does the same, and it matters for dotfiles like `.gitignore`/
        // `.editorconfig`: `Path::extension()` returns `None` for those
        // (Rust treats a leading dot with no further dot as "no
        // extension", not as a hidden file with an empty name).
        // `resolve_syntax_highlighter` falls back to `self.first_line`
        // for files with no usable name at all, like `.git/config`.
        let file_name = self.path.file_name().and_then(|n| n.to_str());
        let extension = self.path.extension().and_then(|e| e.to_str());
        let mut candidates: Vec<&str> = [file_name, extension].into_iter().flatten().collect();

        // A ".sdk" suffix on top of an otherwise-recognizable file name
        // is this project's own build-system convention for a template
        // that becomes the inner file once processed (e.g.
        // "CMakeLists.txt.sdk" is a CMake template) -- also try the
        // name/extension with that one suffix stripped, so these
        // templates get the same highlighting the real file would, on
        // top of (not instead of) the direct candidates above.
        let inner_name = file_name.and_then(|name| name.strip_suffix(".sdk"));
        if let Some(inner_name) = inner_name {
            candidates.push(inner_name);
            if let Some(inner_extension) = Path::new(inner_name).extension().and_then(|e| e.to_str()) {
                candidates.push(inner_extension);
            }
        }

        // Computed once and reused below for the syntax highlighter and
        // for `bracket_match_highlights` -- both would otherwise pay an
        // O(remaining line length) cost against the same pathological
        // line (`has_pathologically_long_line`'s own doc comment).
        let pathologically_long_line = has_pathologically_long_line(&self.state.lines);

        // Skip syntax highlighting entirely for a file with a
        // pathologically long line -- `syntect` tokenizes a line's
        // *full* text on every highlight pass regardless of how much of
        // it is actually visible on screen, so a single enormous line
        // would otherwise pay that cost fresh on every one of this
        // app's per-event redraws (`main.rs::run`).
        let syntax_highlighter = if pathologically_long_line {
            None
        } else {
            resolve_syntax_highlighter(&candidates, &self.first_line, custom_syntax_theme)
        };

        // Widen the viewport to show a multi-line matched bracket pair
        // in full, when it actually fits -- reported directly, with a
        // screenshot: the far bracket only ever highlighted while its
        // own row happened to already be scrolled into view, since
        // `edtui`'s own vertical auto-scroll only ever keeps the
        // *cursor's* row visible, with no notion of "and this other row
        // too" (`matched_bracket_row_span`'s own doc comment). `area`'s
        // height minus 2 approximates edtui's own content height (just
        // the border -- `.hide_status_line()` below means there's no
        // status line to also subtract). Setting `y` here only takes
        // effect if it actually includes the cursor's own row -- `edtui`
        // re-adjusts the offset during render whenever the cursor would
        // otherwise fall outside it (`ViewOffset::update_viewport_vertical`'s
        // own doc comment, confirmed directly from its source), so this
        // can never leave the cursor scrolled out of view even if the
        // math below is wrong. When the pair doesn't fit at all, this
        // deliberately leaves the viewport alone -- keeping the cursor's
        // own row visible (`edtui`'s own default behavior) is the
        // correct fallback, not an error.
        if !pathologically_long_line {
            if let Some((top_row, bottom_row)) = matched_bracket_row_span(&self.state.lines, self.state.cursor) {
                let content_height = area.height.saturating_sub(2) as usize;
                if bottom_row - top_row + 1 <= content_height {
                    let (offset_x, _) = self.state.viewport_offset();
                    self.state.set_viewport_offset(offset_x, top_row);
                }
            }
        }

        let selection_style = Style::default().fg(theme.selection_text.unwrap_or(theme.text)).bg(theme.current_row_bg);

        // VS Code-style "highlight every other occurrence of the word
        // under the cursor" plus Far Manager/VS Code-style bracket-pair
        // matching -- see `word_highlight`'s own doc comment for how
        // this rides `edtui`'s own `state.highlights` field rather than
        // a hand-rolled render pass. Recomputed fresh every frame
        // directly from the cursor's current position -- cheap enough
        // at the file sizes this editor targets (see
        // `word_highlight::word_occurrences`'s own scope note), and
        // avoids tracking a separate "did the cursor move" dirty flag.
        // Skipped entirely while a selection is active, matching VS
        // Code's own behavior -- "the word/bracket under the cursor"
        // isn't a coherent concept mid-selection, and the highlights
        // would just get overridden by the selection's own styling
        // wherever they overlapped anyway (`edtui`'s own priority
        // order: selection, then highlights, then base). Bracket
        // matching is also skipped for a pathologically long line, for
        // the same reason syntax highlighting is above -- see
        // `bracket_match_highlights`'s own doc comment for why it's a
        // wholly separate pass from word-occurrence highlighting, never
        // feeding brackets into "similar" word matches.
        // Same style for word-occurrence and bracket-pair highlighting
        // -- requested directly, after bracket matching first shipped
        // with its own distinct `theme.bg`-on-`theme.accent` look:
        // brackets should read as the same kind of "this matches
        // something nearby" hint as word highlighting, not a visually
        // different feature. Also referenced below, by `cursor_style`,
        // for the same reason.
        let highlight_style = Style::default().fg(theme.text).bg(theme.border);

        self.state.highlights = if self.state.selection.is_none() {
            let mut highlights = word_occurrence_highlights(&self.state.lines, self.state.cursor, highlight_style);
            if !pathologically_long_line {
                highlights.extend(bracket_match_highlights(&self.state.lines, self.state.cursor, highlight_style));
            }
            highlights
        } else {
            Vec::new()
        };

        let mut editor_theme = EditorTheme::default()
            .base(Style::default().fg(theme.text).bg(theme.bg))
            .block(
                Block::bordered()
                    .border_style(Style::default().fg(theme.accent))
                    .title(self.path.to_string_lossy().into_owned()),
            )
            .selection_style(selection_style)
            .hide_status_line()
            // Absolute line numbers, themed to match the rest of the
            // chrome (edtui's own default is a hardcoded black/gray
            // gutter, unrelated to whatever scheme is active) rather
            // than relative — this is a general-purpose text editor,
            // not a modal vim-style one where relative numbers help
            // with `dj`/`5k`-style motions.
            .line_numbers_style(Style::default().fg(theme.text_dim).bg(theme.bg));

        // edtui paints the cursor's own cell *after* selection/highlight
        // styling (`EditorView::render`), unconditionally overwriting
        // whatever color was there -- `.hide_cursor()` only changes that
        // overwrite to `base` instead of leaving it alone, it doesn't
        // skip it. Since this keymap always keeps `state.cursor` exactly
        // on the selection's live end (see `bindings::extend_word_selection`'s
        // doc comment), that one cell is the last character of an active
        // selection -- painting it `base` made it visually look
        // unselected even though it's genuinely included in what `Copy`
        // grabs, reported directly as pasted text having one more
        // character than what looked highlighted. Painting it
        // `selection_style` instead, only while a selection is actually
        // active, keeps the visible highlight and the real selection in
        // agreement.
        //
        // Same reasoning applies to bracket matching: once
        // `bracket_match_highlights` started returning *both* brackets
        // of a pair (not just the far one), the near one -- wherever the
        // cursor itself sits -- was still invisibly overwritten back to
        // plain `base`, reported directly with a screenshot -- the near
        // bracket's own highlight wasn't visible at all, only the far
        // one, even though both are meant to show at once. Painting the
        // cursor cell with `highlight_style` instead, whenever
        // `cursor_is_on_a_matched_bracket` says it's genuinely sitting
        // on one side of a real pair, makes both brackets read as
        // highlighted together the same way selection already does.
        //
        // With no selection and the cursor not on a matched bracket,
        // `hide_cursor()`'s usual `base` is right: the real terminal
        // cursor (a thin bar — see `setup_terminal` in main.rs) is what
        // should be visible there, not edtui's own solid reverse-video
        // block.
        editor_theme = if self.state.selection.is_some() {
            editor_theme.cursor_style(selection_style)
        } else if !pathologically_long_line && cursor_is_on_a_matched_bracket(&self.state.lines, self.state.cursor) {
            editor_theme.cursor_style(highlight_style)
        } else {
            editor_theme.hide_cursor()
        };

        EditorView::new(&mut self.state)
            .theme(editor_theme)
            .syntax_highlighter(syntax_highlighter)
            .line_numbers(LineNumbers::Absolute)
    }


    /// Where the real terminal cursor should be positioned to sit on
    /// top of the character currently under edit — `None` if the
    /// cursor is currently scrolled out of view. Only meaningful after
    /// `view()` has actually been rendered this frame (it computes this
    /// as part of rendering).
    ///
    /// While extending a selection *forward* (growing rightward/downward
    /// from where it started), shifted one column *right* of
    /// `state.cursor`'s own cell -- `state.cursor` sits exactly on the
    /// selection's live end (see `bindings::extend_word_selection`'s
    /// doc comment for why that invariant is load-bearing), but a
    /// terminal's own bar-shaped cursor is drawn at the *left* edge of
    /// whatever cell it's positioned on. Left unshifted, the bar renders
    /// on the boundary between the last selected character and the one
    /// before it -- reads as "the selection stops one character early"
    /// even though the highlighted cell and what `Copy` grabs are both
    /// already correct (confirmed directly: real logs showed the last
    /// selected character's cell genuinely painted with the selection
    /// color and genuinely included in the copied text -- only the
    /// blinking bar's own screen position was misleading). Shifting it
    /// one column right puts the bar on the boundary *after* the last
    /// selected character instead, matching where a caret sits at the
    /// end of a selection in every other editor.
    ///
    /// While extending *backward* (growing leftward/upward), the cursor
    /// sits at the *earliest* end of the selection instead, not the
    /// latest -- shifting right there put the bar on the boundary
    /// *after* the first selected character rather than before it,
    /// reported directly against real text (retracting onto the `'l'`
    /// of "loaded" rendered the bar between `'l'` and `'o'`, reading as
    /// if `'l'` itself weren't selected, even though it genuinely was).
    /// So the shift only applies when the cursor is at or after the
    /// selection's own `start` in reading order (row, then column) --
    /// i.e. only while it's the *trailing* edge of the selection, which
    /// is exactly the forward-extension case above. Plain typing (no
    /// selection) is unaffected either way -- the cursor already sits
    /// exactly where the next typed character would land, no shift
    /// needed there.
    pub fn cursor_screen_position(&self) -> Option<ratatui::layout::Position> {
        let mut pos = self.state.cursor_screen_position()?;
        if let Some(selection) = &self.state.selection {
            let cursor_is_trailing_edge = (self.state.cursor.row, self.state.cursor.col) >= (selection.start.row, selection.start.col);
            if cursor_is_trailing_edge {
                pos.x = pos.x.saturating_add(1);
            }
        }
        Some(pos)
    }
}


#[cfg(test)]
mod tests;
