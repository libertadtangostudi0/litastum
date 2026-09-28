use std::path::Path;

use edtui::{EditorTheme, EditorView, Highlight, LineNumbers};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Block;

use crate::theming::Theme;

use super::super::bracket_match::{bracket_match_highlights, cursor_is_on_a_matched_bracket, matched_bracket_row_span};
use super::super::syntax::resolve_syntax_highlighter;
use super::super::word_highlight::{has_pathologically_long_line, word_occurrence_highlights};
use super::Editor;

impl Editor {
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
        // app's per-event redraws (`event_loop::run`).
        let syntax_highlighter = if pathologically_long_line || !self.syntax_highlighting_enabled {
            None
        } else {
            resolve_syntax_highlighter(&syntax_name_candidates(&self.path), &self.first_line, &self.custom_syntax_theme)
        };

        if !pathologically_long_line {
            self.widen_viewport_to_show_matched_bracket_pair(area);
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

        // While the `Ctrl+F` box is open, its current match is the only
        // highlight shown (plus `extra_highlights`, which search never
        // coexists with -- Compare doesn't route keys through the search
        // box at all). `edtui` used to draw the match itself, as a
        // synthetic selection built from its own `SearchState`; this
        // app's own `SearchSession` replaced that (see its doc comment),
        // so the match is a plain `Highlight` now -- and `edtui`'s two
        // render paths disagree on which of two overlapping highlights
        // wins (first in the plain path, last in the syntax-highlighted
        // one), so word-occurrence/bracket highlights are left out
        // entirely rather than risk one of them painting over the match.
        self.state.highlights = if let Some((start, end)) = self.search_match_span() {
            vec![Highlight::new(start, end, selection_style)]
        } else if self.state.selection.is_none() {
            let mut highlights = word_occurrence_highlights(&self.state.lines, self.state.cursor, rows_that_can_be_visible(self.state.cursor.row, area), highlight_style);
            if !pathologically_long_line {
                highlights.extend(bracket_match_highlights(&self.state.lines, self.state.cursor, highlight_style));
            }
            highlights
        } else {
            Vec::new()
        };
        self.state.highlights.extend(self.extra_highlights.iter().cloned());

        let editor_theme = EditorTheme::default()
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
        let cursor_cell_style = self.cursor_cell_style(selection_style, highlight_style, pathologically_long_line);
        let editor_theme = match cursor_cell_style {
            Some(style) => editor_theme.cursor_style(style),
            None => editor_theme.hide_cursor(),
        };

        EditorView::new(&mut self.state)
            .theme(editor_theme)
            .syntax_highlighter(syntax_highlighter)
            .line_numbers(LineNumbers::Absolute)
    }

    /// Widen the viewport to show a multi-line matched bracket pair in
    /// full, when it actually fits -- reported directly, with a
    /// screenshot: the far bracket only ever highlighted while its own
    /// row happened to already be scrolled into view, since `edtui`'s
    /// own vertical auto-scroll only ever keeps the *cursor's* row
    /// visible, with no notion of "and this other row too"
    /// (`matched_bracket_row_span`'s own doc comment). `area`'s height
    /// minus 2 approximates edtui's own content height (just the border
    /// -- `.hide_status_line()` in `view` means there's no status line
    /// to also subtract). Setting `y` here only takes effect if it
    /// actually includes the cursor's own row -- `edtui` re-adjusts the
    /// offset during render whenever the cursor would otherwise fall
    /// outside it (`ViewOffset::update_viewport_vertical`'s own doc
    /// comment, confirmed directly from its source), so this can never
    /// leave the cursor scrolled out of view even if the math below is
    /// wrong. When the pair doesn't fit at all, this deliberately
    /// leaves the viewport alone -- keeping the cursor's own row
    /// visible (`edtui`'s own default behavior) is the correct
    /// fallback, not an error.
    fn widen_viewport_to_show_matched_bracket_pair(&mut self, area: Rect) {
        let Some((top_row, bottom_row)) = matched_bracket_row_span(&self.state.lines, self.state.cursor) else {
            return;
        };
        let content_height = area.height.saturating_sub(2) as usize;
        if bottom_row - top_row < content_height {
            let (offset_x, _) = self.state.viewport_offset();
            self.state.set_viewport_offset(offset_x, top_row);
        }
    }

    /// The style `edtui` should paint the cursor's own cell with, or
    /// `None` for `hide_cursor()`'s plain `base`.
    ///
    /// edtui paints the cursor's own cell *after* selection/highlight
    /// styling (`EditorView::render`), unconditionally overwriting
    /// whatever color was there -- `.hide_cursor()` only changes that
    /// overwrite to `base` instead of leaving it alone, it doesn't skip
    /// it. So every kind of highlight the cursor can sit inside needs
    /// its own exception here, or that one cell gets visibly cut out of
    /// it. Each was reported directly, one at a time:
    ///
    /// - **Selection**: this keymap always keeps `state.cursor` exactly
    ///   on the selection's live end (see `bindings::extend_word_selection`'s
    ///   doc comment), so that cell is the last character of an active
    ///   selection -- painting it `base` made pasted text look like it
    ///   had one more character than what was highlighted.
    /// - **Bracket matching**: once `bracket_match_highlights` returned
    ///   *both* brackets of a pair, the near one (under the cursor) was
    ///   still overwritten, so only the far one ever looked highlighted.
    /// - **`Ctrl+F` search**: the current match is painted in
    ///   `selection_style` too (`view`'s own search highlight), but a
    ///   search jump puts the cursor on the match's *first* character --
    ///   which then never looked highlighted. Reported back when `edtui`
    ///   still drew the match itself; `search::SearchSession` keeps the
    ///   same cursor placement, so the exception still applies.
    /// - **`extra_highlights`** (Compare's red/green diff rows,
    ///   `ui/compare.rs::row_highlights`): the cursor's cell on a
    ///   diff-colored line rendered as a plain patch cut out of it.
    ///   Reuses whichever *specific* highlight the cursor sits inside
    ///   (`Highlight::contains`) -- a diff row can be either the removed
    ///   or the added color, so hardcoding one would just trade one
    ///   wrong color for another. Plain `F4` editing never sets
    ///   `extra_highlights`, so this is a Compare-only case.
    ///
    /// With none of these applying, `None` is right: the real terminal
    /// cursor (a thin bar — see `terminal_setup::setup_terminal`) is what
    /// should be visible there, not edtui's own solid reverse-video
    /// block.
    fn cursor_cell_style(&self, selection_style: Style, highlight_style: Style, pathologically_long_line: bool) -> Option<Style> {
        if self.state.selection.is_some() || self.is_searching() {
            return Some(selection_style);
        }
        if !pathologically_long_line && cursor_is_on_a_matched_bracket(&self.state.lines, self.state.cursor) {
            return Some(highlight_style);
        }
        self.extra_highlights.iter().find(|highlight| highlight.contains(&self.state.cursor)).map(|highlight| highlight.style)
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


/// Every buffer row that could possibly end up on screen this frame --
/// `edtui` always keeps the cursor's own row visible, so whatever
/// viewport it settles on lies within one screen height of it in either
/// direction (the bracket-pair viewport nudge in `view` only ever
/// applies when the whole pair fits, which keeps the cursor on screen
/// too). `area`'s height is an upper bound on the real content height
/// (border and gutter only make it smaller).
fn rows_that_can_be_visible(cursor_row: usize, area: Rect) -> std::ops::Range<usize> {
    let height = area.height as usize;
    cursor_row.saturating_sub(height)..cursor_row + height + 1
}


/// Names `resolve_syntax_highlighter` should try for `path`, in order.
///
/// The full file name first, then just the extension — `syntect`'s own
/// convenience lookup (`SyntaxSet::find_syntax_for_file`) does the same,
/// and it matters for dotfiles like `.gitignore`/`.editorconfig`:
/// `Path::extension()` returns `None` for those (Rust treats a leading
/// dot with no further dot as "no extension", not as a hidden file with
/// an empty name). `resolve_syntax_highlighter` falls back to the file's
/// first line for files with no usable name at all, like `.git/config`.
///
/// A ".sdk" suffix on top of an otherwise-recognizable file name is this
/// project's own build-system convention for a template that becomes
/// the inner file once processed (e.g. "CMakeLists.txt.sdk" is a CMake
/// template) -- also try the name/extension with that one suffix
/// stripped, so these templates get the same highlighting the real file
/// would, on top of (not instead of) the direct candidates.
fn syntax_name_candidates(path: &Path) -> Vec<&str> {
    let file_name = path.file_name().and_then(|n| n.to_str());
    let extension = path.extension().and_then(|e| e.to_str());
    let mut candidates: Vec<&str> = [file_name, extension].into_iter().flatten().collect();

    if let Some(inner_name) = file_name.and_then(|name| name.strip_suffix(".sdk")) {
        candidates.push(inner_name);
        if let Some(inner_extension) = Path::new(inner_name).extension().and_then(|e| e.to_str()) {
            candidates.push(inner_extension);
        }
    }
    candidates
}
