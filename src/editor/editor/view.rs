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
    /// Builds this frame's view: content, syntax highlighting (skipped if
    /// no grammar matches), highlights and theme. `&mut self` because
    /// `EditorView` updates scrolling while drawing; `area` is the rect
    /// about to be rendered into, needed to fit a bracket pair on screen.
    pub fn view(&mut self, theme: &Theme, area: Rect) -> EditorView<'_, '_> {
        self.view_area = area;
        // Computed once and reused below for the syntax highlighter and
        // for `bracket_match_highlights` -- both would otherwise pay an
        // O(remaining line length) cost against the same pathological
        // line (`has_pathologically_long_line`'s own doc comment).
        let pathologically_long_line = has_pathologically_long_line(&self.state.lines);

        // `syntect` tokenizes whole lines on every redraw, visible or not.
        let syntax_highlighter = if pathologically_long_line || !self.syntax_highlighting_enabled {
            None
        } else {
            resolve_syntax_highlighter(&syntax_name_candidates(&self.path), &self.first_line, &self.custom_syntax_theme)
        };

        if !pathologically_long_line {
            self.widen_viewport_to_show_matched_bracket_pair(area);
        }

        let selection_style = Style::default().fg(theme.selection_text.unwrap_or(theme.text)).bg(theme.current_row_bg);

        // Word occurrences and bracket pairs share one style, recomputed
        // each frame from the cursor. Skipped during a selection (VS Code
        // does the same), and brackets for a pathologically long line.
        let highlight_style = Style::default().fg(theme.text).bg(theme.border);

        // With the search box open, its match is the only highlight:
        // `edtui`'s two render paths disagree on which overlapping
        // highlight wins. History: docs/history/editor-rendering.md.
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
            // Absolute, themed line numbers (edtui's default gutter
            // ignores the scheme).
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

    /// Moves the viewport so a multi-line bracket pair is fully visible
    /// when it fits -- `edtui` only keeps the cursor's row in view.
    /// `edtui` re-adjusts if the cursor would fall outside, so this can't
    /// hide it. `area.height - 2` = content height (border only, no
    /// status line).
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
    /// `edtui` paints the cursor cell after every highlight, so each
    /// highlight the cursor can be inside gets the matching style here:
    /// the selection (the cursor is on its live end), the search match
    /// the caret is inside, a matched bracket, or the `extra_highlights`
    /// entry (Compare's diff row) it's in. Otherwise `None`: the terminal
    /// bar cursor shows, not edtui's block. History: docs/history/editor-rendering.md.
    fn cursor_cell_style(&self, selection_style: Style, highlight_style: Style, pathologically_long_line: bool) -> Option<Style> {
        let caret_in_search_match =
            self.search_match_span().is_some_and(|(start, end)| self.state.cursor.row == start.row && (start.col..=end.col).contains(&self.state.cursor.col));
        if self.state.selection.is_some() || caret_in_search_match {
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
    /// On a selection's trailing edge it's shifted one column right: a
    /// bar cursor draws at its cell's left edge, so on the last selected
    /// character it looked like the selection stopped one short. Not on
    /// the leading edge (extending backward), where the shift would do
    /// the opposite. History: docs/history/editor-rendering.md.
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
/// The full file name first, then the extension -- `Path::extension()`
/// is `None` for dotfiles like `.gitignore`. A trailing `.sdk` (this
/// project's build-template convention, `CMakeLists.txt.sdk`) is also
/// tried stripped.
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
