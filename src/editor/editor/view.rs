use std::path::Path;

use edtui::{EditorTheme, EditorView, Highlight, LineNumbers, Lines, RowIndex};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Block;

use crate::theming::Theme;

use super::super::bracket_match::{bracket_match_highlights, cursor_is_on_a_matched_bracket, matched_bracket_row_span};
use super::super::syntax::resolve_syntax_highlighter;
use super::super::word_highlight::word_occurrence_highlights;
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
        let pathologically_long_line = self.has_long_line;

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
        let visible = super::highlights_on(&self.extra_highlights, rows_that_can_be_visible(self.state.cursor.row, area));
        self.state.highlights.extend(visible.iter().cloned());

        let editor_theme = EditorTheme::default()
            .base(Style::default().fg(theme.text).bg(theme.bg))
            .block(
                Block::bordered()
                    .border_style(Style::default().fg(theme.accent))
                    .title(self.title(area.width)),
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

    /// The top row that puts `row` in the middle of the view, measured in
    /// screen rows: `edtui` wraps long lines, so counting buffer rows put
    /// the target below the middle (and `edtui` then scrolled again).
    /// Clamped so the view never runs past the last line. `None` before
    /// the first draw (no size known yet).
    pub(super) fn centered_top_row(&self, row: usize) -> Option<usize> {
        let content_height = self.view_area.height.saturating_sub(2) as usize;
        let text_width = self.text_width();
        if content_height == 0 || text_width == 0 {
            return None;
        }
        let height_of = |row: usize| screen_rows(&self.state.lines, row, text_width);

        let target = height_of(row).min(content_height);
        let wanted_above = (content_height - target + 1) / 2;
        let mut top = row;
        let mut above = 0;
        while top > 0 && above + height_of(top - 1) <= wanted_above {
            top -= 1;
            above += height_of(top);
        }

        // The lowest top that still fills the view down to the last line.
        let mut last_top = self.state.lines.len();
        let mut filled = 0;
        while last_top > 0 && filled < content_height {
            last_top -= 1;
            filled += height_of(last_top);
        }
        if filled > content_height {
            last_top += 1;
        }
        Some(top.min(last_top))
    }

    /// Columns available for text: the view minus its border and the
    /// line-number gutter (the widest number plus a space).
    pub(super) fn text_width(&self) -> usize {
        (self.view_area.width as usize).saturating_sub(2 + self.gutter_width())
    }

    /// The screen column the text starts at: past the left border and the
    /// line-number gutter.
    pub(super) fn text_left(&self) -> u16 {
        self.view_area.x.saturating_add(1).saturating_add(self.gutter_width() as u16)
    }

    /// The line-number gutter: the widest number plus a space.
    fn gutter_width(&self) -> usize {
        self.state.lines.len().max(1).to_string().len() + 1
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
        let row = self.state.cursor.row;
        super::highlights_on(&self.extra_highlights, row..row + 1).iter().find(|highlight| highlight.contains(&self.state.cursor)).map(|highlight| highlight.style)
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
    /// the opposite, nor on a line break (`(row, len)`, a selection of
    /// whole lines), which has no character to step past.
    /// History: docs/history/editor-rendering.md.
    pub fn cursor_screen_position(&self) -> Option<ratatui::layout::Position> {
        let mut pos = self.state.cursor_screen_position()?;
        if let Some(selection) = &self.state.selection {
            let cursor_is_trailing_edge = (self.state.cursor.row, self.state.cursor.col) >= (selection.start.row, selection.start.col);
            let on_line_break = self.state.cursor.col >= self.state.lines.len_col(self.state.cursor.row).unwrap_or(0);
            if cursor_is_trailing_edge && !on_line_break {
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


/// How many screen rows buffer `row` takes when wrapped at `text_width`
/// (`edtui` wraps by character): at least one, even when empty.
fn screen_rows(lines: &Lines, row: usize, text_width: usize) -> usize {
    let len = lines.get(RowIndex::new(row)).map_or(0, |line| line.len());
    len.div_ceil(text_width).max(1)
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


impl Editor {
    /// The top border's title: the path, then `[modified]` while there
    /// are unsaved changes (it used to sit in a hint row under the editor,
    /// now gone). The marker always fits; the path gives way.
    fn title(&self, area_width: u16) -> String {
        let marker = if self.is_dirty() { " [modified]" } else { "" };
        let path = fitted_title(&self.path, area_width.saturating_sub(marker.len() as u16));
        format!("{path}{marker}")
    }
}


/// The path for the top border, `area_width` wide: a path too long to fit
/// keeps its end -- the file name -- behind a leading `…`.
pub(crate) fn fitted_title(path: &Path, area_width: u16) -> String {
    let title = path.to_string_lossy();
    let room = usize::from(area_width.saturating_sub(2));
    let length = title.chars().count();
    if length <= room {
        return title.into_owned();
    }
    let tail: String = title.chars().skip(length - room.saturating_sub(1)).collect();
    format!("…{tail}")
}


#[cfg(test)]
mod fitted_title_tests {
    use super::*;

    #[test]
    fn a_path_that_fits_is_shown_whole() {
        assert_eq!(fitted_title(Path::new("src/a.rs"), 12), "src/a.rs");
    }

    /// Reported: a long path showed only its start, hiding the file name.
    #[test]
    fn a_long_path_keeps_its_end() {
        let title = fitted_title(Path::new("W:/WorkCopies/lib/IcEdSelectionSetImpl.h"), 25);
        assert_eq!(title, "…IcEdSelectionSetImpl.h");
        assert_eq!(title.chars().count(), 23, "the width minus the two corners");
    }
}


#[cfg(test)]
mod title_tests {
    use crossterm::event::KeyCode;

    use crate::editor::EditorKeymapMode;
    use crate::test_support::{key, unique_scratch_dir};

    use super::super::Editor;

    /// `[modified]` moved from the removed hint row into the title, and
    /// stays whole when the path has to be shortened.
    #[test]
    fn the_title_says_modified_after_an_edit() {
        let path = unique_scratch_dir("editor-title").join("file.txt");
        std::fs::write(&path, "a\n").unwrap();
        let mut editor = Editor::open(path, None, EditorKeymapMode::Standard).unwrap();
        assert!(!editor.title(200).contains("[modified]"));

        editor.input(key(KeyCode::Char('x')));

        assert!(editor.title(200).ends_with("file.txt [modified]"));
        let narrow = editor.title(22);
        assert!(narrow.ends_with(" [modified]") && narrow.chars().count() <= 20, "{narrow}");
    }
}
