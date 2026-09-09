use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};
use edtui::syntect::highlighting::Theme as SynTheme;
use edtui::{EditorEventHandler, EditorMode, EditorState, EditorTheme, EditorView, LineNumbers, Lines};
use ratatui::style::Style;
use ratatui::widgets::Block;
use tracing::{debug, warn};

use crate::theming::Theme;

use super::bindings::{
    anchor_fresh_shift_selection, close_selection_if_back_on_the_anchors_row, exclude_landing_column_on_fresh_vertical_selection,
    standard_key_handler, wrap_line_boundary_arrow_movement,
};
use super::clipboard::OsClipboardBridge;
use super::syntax::resolve_syntax_highlighter;

/// A single open-file editing session, backed by `edtui`. Owns the path
/// it was loaded from (for `save`) and a snapshot of the content as of
/// the last load/save (for `is_dirty`, computed by comparing the
/// current buffer to it — simpler and more accurate than tracking a
/// hand-maintained dirty flag, since it self-corrects if the user
/// undoes their way back to a saved state).
pub struct Editor {
    path: PathBuf,
    state: EditorState,
    event_handler: EditorEventHandler,
    saved_snapshot: Lines,
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
    /// the *current* selection so far -- the signal `bindings::
    /// extend_word_selection` needs to tell "a `Left` press should
    /// retract fully" apart from "a `Left` press is genuinely walking
    /// backward through fresh text, nothing to retract," which turned
    /// out not to be reliably derivable from character classification
    /// alone (see that function's own doc comment, "Eighth," for two
    /// real reports a character-based guess got wrong in two different
    /// ways). See `WordSelectTouch`'s own doc comment for what each
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
}

/// See `Editor::word_select_touch`'s own doc comment for why this
/// exists. Three states, not two (`Option<bool>`/"has it gone forward
/// yet"), because a plain boolean can't tell "word-wise selection has
/// never touched this selection at all" (`Untouched` -- e.g. it was
/// built by character-wise `Shift+Right`, a mouse drag, or anything
/// else that isn't `extend_word_selection`) apart from "word-wise
/// selection built this whole thing itself via repeated backward
/// presses" (`NativeBackward`) -- confirmed the hard way: a selection
/// built by *anything other than* word-wise `Right` presses (real
/// report: a whole line selected some other way, then trimmed with
/// `Ctrl+Shift+Left`) needs the *same* full retraction `Untouched`
/// wants, but an `Option<bool>` collapsing both of those into one value
/// can't tell them apart from `NativeBackward`'s own "keep walking
/// backward through nothing already selected" case, which must *not*
/// retract (`repeated_left_monotonically_extends_through_punctuation`,
/// unaffected on purpose).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WordSelectTouch {
    /// No selection at all, or one exists but word-wise selection
    /// hasn't acted on it yet. A backward press from here should
    /// retract fully -- there's real content to shrink away from,
    /// word-wise selection just hasn't touched it before.
    Untouched,
    /// The *current* selection was built by word-wise selection itself,
    /// starting with a backward (`Left`) press, and every press since
    /// has also been backward -- a pure walk backward through fresh
    /// text extending the selection, not retracting anything.
    NativeBackward,
    /// Word-wise selection has done at least one forward (`Right`)
    /// press on the current selection, or at least one retraction --
    /// a backward press from here is undoing part of that.
    Touched,
    // Known, narrow gap, not chased further: `Editor::extend_word_selection`
    // never resets this back to `Untouched` -- once any word-wise press
    // happens, it's `Touched`/`NativeBackward` for good, since there's no
    // hook here for "the selection was closed and a *different* one was
    // built some other way" (that happens entirely inside `Editor::input`,
    // outside this type's view). In practice this only matters if an
    // earlier word-wise session ended in `NativeBackward` *and* a later,
    // entirely separate selection (built without word-wise selection ever
    // touching it) is then retracted with `Ctrl+Shift+Left` as its very
    // first action -- `Touched` left over instead gives the right answer
    // anyway, since `Touched` and `Untouched` both retract; only a leftover
    // `NativeBackward` would wrongly skip it. Narrow enough (two unrelated
    // things have to line up) not to be worth a bigger hook for yet.
}


impl Editor {
    /// Loads `path`'s contents into a new editing session. Fails if the
    /// file can't be read as UTF-8 text (binary files aren't supported
    /// yet — see `TODO.md`). `custom_syntax_theme` is `None` for the
    /// built-in named syntax theme, or a scheme-derived theme when the
    /// user has a custom color scheme configured.
    pub fn open(path: PathBuf, custom_syntax_theme: Option<SynTheme>) -> io::Result<Self> {
        let contents = fs::read_to_string(&path)?;
        let lines = Lines::from(contents.as_str());
        let first_line = contents.lines().next().unwrap_or("").to_string();

        let mut state = EditorState::new(lines.clone());
        state.mode = EditorMode::Insert;
        state.set_clipboard(OsClipboardBridge);

        Ok(Self {
            path,
            state,
            event_handler: EditorEventHandler::new(standard_key_handler()),
            saved_snapshot: lines,
            custom_syntax_theme,
            first_line,
            word_select_touch: WordSelectTouch::Untouched,
            vertical_shift_anchor_col: None,
        })
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


    /// `Ctrl+Shift+Left`/`Right` -- word-wise selection. Not part of
    /// `input`'s own dispatch (`editor_keymap.rs::handle_editor_key`
    /// calls this directly instead) -- see `bindings::extend_word_selection`'s
    /// own doc comment for why this needed real logic of its own rather
    /// than another entry in `standard_key_handler`'s declarative table.
    ///
    /// Owns `word_select_touch` -- reads it (as `retracting`, "should
    /// this `Left` press retract fully") before this press changes
    /// anything, then updates it for next time. `retracting` is only
    /// ever `true` while there's an existing selection (`!fresh`) this
    /// press is going backward against (`!forward`) *and* the selection
    /// isn't a pure `NativeBackward` walk -- both `Untouched` (word-wise
    /// selection has never acted on it -- built some other way, or this
    /// is the very first backward touch of it) and `Touched` (word-wise
    /// selection has gone forward, or already retracted, at least once)
    /// count, which is exactly what lets both real reports in
    /// `extend_word_selection`'s own doc comment ("Eighth") retract
    /// correctly -- one starting from a word-wise `Right`-built
    /// selection, the other from one built some other way entirely.
    pub fn extend_word_selection(&mut self, forward: bool) {
        let fresh = self.state.mode != EditorMode::Visual;
        let retracting = !fresh && !forward && self.word_select_touch != WordSelectTouch::NativeBackward;

        super::bindings::extend_word_selection(&mut self.state, forward, retracting);

        self.word_select_touch = match (fresh, forward, self.word_select_touch) {
            (true, true, _) => WordSelectTouch::Touched,
            (true, false, _) => WordSelectTouch::NativeBackward,
            (false, true, _) => WordSelectTouch::Touched,
            (false, false, WordSelectTouch::NativeBackward) => WordSelectTouch::NativeBackward,
            (false, false, _) => WordSelectTouch::Touched,
        };
    }


    /// Whether there's an active text selection (used by the caller to
    /// decide whether `Esc` should cancel the selection or close the
    /// editor).
    pub fn has_selection(&self) -> bool {
        self.state.selection.is_some()
    }


    /// Writes the current buffer back to the file it was opened from.
    pub fn save(&mut self) -> io::Result<()> {
        let contents = String::from(self.state.lines.clone());
        debug!(path = %self.path.display(), bytes = contents.len(), "editor save: writing");
        match fs::write(&self.path, &contents) {
            Ok(()) => {
                self.saved_snapshot = self.state.lines.clone();
                debug!(path = %self.path.display(), "editor save: ok");
                Ok(())
            }
            Err(err) => {
                warn!(path = %self.path.display(), %err, "editor save: failed");
                Err(err)
            }
        }
    }


    /// Whether the buffer differs from the last loaded/saved snapshot.
    pub fn is_dirty(&self) -> bool {
        self.state.lines != self.saved_snapshot
    }


    /// Builds this frame's renderable view: the editor content plus
    /// syntax highlighting (best-effort — silently skipped if nothing
    /// recognizes this file, by name or by first line) and our theme.
    ///
    /// Takes `&mut self`, unlike a typical read-only render helper:
    /// `EditorView` tracks scroll position as part of rendering, so it
    /// needs write access to `EditorState` even just to draw.
    pub fn view(&mut self, theme: &Theme) -> EditorView<'_, '_> {
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

        let syntax_highlighter = resolve_syntax_highlighter(&candidates, &self.first_line, custom_syntax_theme);
        debug!(?candidates, found = syntax_highlighter.is_some(), "syntax highlighter lookup");

        let selection_style = Style::default().fg(theme.text).bg(theme.current_row_bg);

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

        // edtui paints the cursor's own cell *after* selection styling
        // (`EditorView::render`), unconditionally overwriting whatever
        // color was there -- `.hide_cursor()` only changes that overwrite
        // to `base` instead of leaving it alone, it doesn't skip it. Since
        // this keymap always keeps `state.cursor` exactly on the
        // selection's live end (see `bindings::extend_word_selection`'s
        // doc comment), that one cell is the last character of an active
        // selection -- painting it `base` made it visually look
        // unselected even though it's genuinely included in what `Copy`
        // grabs, reported directly as pasted text having one more
        // character than what looked highlighted. Painting it
        // `selection_style` instead, only while a selection is actually
        // active, keeps the visible highlight and the real selection in
        // agreement. With no selection, `hide_cursor()`'s usual `base` is
        // right: the real terminal cursor (a thin bar — see
        // `setup_terminal` in main.rs) is what should be visible there,
        // not edtui's own solid reverse-video block.
        editor_theme = if self.state.selection.is_some() {
            editor_theme.cursor_style(selection_style)
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
