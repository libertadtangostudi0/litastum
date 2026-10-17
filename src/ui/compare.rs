use edtui::{Highlight, Index2};
use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::compare::{inline_changes, map_real_row, DiffLines, CompareState, DiffLineKind, LineEnding, LineEndingDisplay, Side};
use crate::editor::Editor;
use crate::path_edit::PathEdit;
use crate::theming::Theme;

/// `Alt+F5`: two ordinary, editable `Editor` panes side by side, with
/// red/green diff backgrounds (`Editor::set_extra_highlights`) and a hint
/// row. The diff is recomputed every frame from the live text; its
/// `Empty` padding rows are never drawn. Only the focused pane scrolls
/// itself -- the other is aligned every frame via `map_real_row` and
/// `Editor::set_viewport_top_row`. History: docs/history/compare.md.
pub(super) fn draw_compare(frame: &mut Frame, area: Rect, state: &mut CompareState, theme: &Theme, line_ending_display: LineEndingDisplay) -> Option<Position> {
    // No hint row: requested, the screen is the panes'.
    draw_compare_panes(frame, area, state, theme, line_ending_display)
}

/// The two diffed panes -- also the conflict
/// resolver's bottom half. Returns the focused pane's cursor.
pub(super) fn draw_compare_panes(frame: &mut Frame, area: Rect, state: &mut CompareState, theme: &Theme, line_ending_display: LineEndingDisplay) -> Option<Position> {
    let panes = Layout::default().direction(Direction::Horizontal).constraints([Constraint::Percentage(50), Constraint::Percentage(50)]).split(area);

    let CompareState { left, right, diff, highlight_colors, focus, left_line_endings, right_line_endings, path_edit, .. } = state;
    let ((left_diff, right_diff), fresh) = diff.get(left, right);
    // Rebuilt only with the diff (or the theme): moving the caret used to
    // rebuild every changed row's highlight each frame.
    let colors = (theme.diff_removed_bg, theme.diff_added_bg);
    if fresh || *highlight_colors != Some(colors) {
        *highlight_colors = Some(colors);
        let removed = ChangeColors::from(theme.danger, theme.diff_removed_bg, theme);
        let added = ChangeColors::from(theme.success, theme.diff_added_bg, theme);
        let left_highlights = row_highlights(left_diff, right_diff, true, left, right, removed, theme);
        let right_highlights = row_highlights(right_diff, left_diff, false, right, left, added, theme);
        left.set_extra_highlights(left_highlights);
        right.set_extra_highlights(right_highlights);
    }

    // The focused pane is drawn first: drawing scrolls it to its caret,
    // and the other pane follows that scroll in the same frame -- aligned
    // before it, the other pane lagged a key behind (requested).
    let left_pane = Pane { editor: left, diff: left_diff, line_endings: left_line_endings, area: panes[0] };
    let right_pane = Pane { editor: right, diff: right_diff, line_endings: right_line_endings, area: panes[1] };
    let (focused, other) = match focus {
        Side::Left => (left_pane, right_pane),
        Side::Right => (right_pane, left_pane),
    };
    let cursor = draw_pane(frame, focused.area, focused.editor, theme, line_ending_display, focused.line_endings, path_edit.as_ref());
    let top = map_real_row(&focused.diff.source_index, &other.diff.source_index, focused.editor.viewport_top_row());
    other.editor.set_viewport_top_row(top);
    draw_pane(frame, other.area, other.editor, theme, line_ending_display, other.line_endings, None);
    cursor
}


/// One side of Compare, as drawing needs it.
struct Pane<'a> {
    editor: &'a mut Editor,
    diff: &'a DiffLines,
    line_endings: &'a [Option<LineEnding>],
    area: Rect,
}

/// Draws one pane; returns where its caret would go if it has focus.
fn draw_pane(frame: &mut Frame, area: Rect, editor: &mut Editor, theme: &Theme, line_ending_display: LineEndingDisplay, line_endings: &[Option<LineEnding>], path_edit: Option<&PathEdit>) -> Option<Position> {
    frame.render_widget(editor.view(theme, area), area);
    // After drawing, which scrolls the view to the caret.
    let viewport_top_row = editor.viewport_top_row();
    let mut cursor = editor.cursor_screen_position();
    if line_ending_display == LineEndingDisplay::Shown {
        draw_line_ending_overlay(frame, area, line_endings, viewport_top_row, theme);
    }
    if let Some(edit) = path_edit {
        cursor = super::path_edit::draw_path_field(frame, editor.title_area(), edit, theme);
    }
    cursor
}

/// The two backgrounds of one side's changes: `change` for what differs,
/// `line` (fainter) for the rest of a changed line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ChangeColors {
    pub line: Color,
    pub change: Color,
}

impl ChangeColors {
    /// `change` as given (the theme's diff color); `line` the same hue,
    /// blended fainter over the background.
    pub fn from(accent: Color, change: Color, theme: &Theme) -> Self {
        Self { line: crate::theming::blend_over_bg(accent, theme.bg, 0.12), change }
    }
}


/// One side's `Highlight`s, `this` diffed against `other` (row-aligned).
/// A line added or removed as a whole is `change`-colored throughout. A
/// changed line with a counterpart on the other side (the same row of a
/// replaced block) is `line`-colored, with only the characters that
/// differ (`inline_changes`) in `change` -- reported: the whole line
/// lit up when one character changed. `this_is_old`: `this` is the left
/// side of `compute`. The character highlights come first: Compare's
/// panes (no syntax colors) let the first of two overlapping highlights
/// win.
pub(super) fn row_highlights(this: &DiffLines, other: &DiffLines, this_is_old: bool, this_editor: &Editor, other_editor: &Editor, colors: ChangeColors, theme: &Theme) -> Vec<Highlight> {
    let style = |bg: Color| Style::default().fg(theme.text).bg(bg);
    let span = |row: usize, columns: std::ops::Range<usize>, bg: Color| Highlight::new(Index2::new(row, columns.start), Index2::new(row, columns.end.saturating_sub(1)), style(bg));
    let mut highlights = Vec::new();
    for (diff_row, kind) in this.kinds.iter().enumerate() {
        if !matches!(kind, DiffLineKind::Removed | DiffLineKind::Added) {
            continue;
        }
        let Some(real_row) = this.source_index[diff_row] else { continue };
        let line_len = this_editor.line_len(real_row);
        let counterpart = other.source_index.get(diff_row).copied().flatten().filter(|_| matches!(other.kinds[diff_row], DiffLineKind::Removed | DiffLineKind::Added));
        match counterpart {
            Some(other_row) => {
                let (this_text, other_text) = (this_editor.line_text(real_row), other_editor.line_text(other_row));
                let (old, new) = if this_is_old { inline_changes(&this_text, &other_text) } else { inline_changes(&other_text, &this_text) };
                let changes = if this_is_old { old } else { new };
                highlights.extend(changes.into_iter().map(|columns| span(real_row, columns, colors.change)));
                highlights.push(span(real_row, 0..line_len, colors.line));
            }
            None => highlights.push(span(real_row, 0..line_len, colors.change)),
        }
    }
    highlights
}

/// Right-aligned `[CRLF]`/`[LF]` markers drawn over the editor view --
/// never inserted into the buffer, which is saved to disk.
/// `line_endings` is the snapshot taken on open (`edtui` drops `\r`).
/// `inner` approximates the view's 1-cell border; only its right edge
/// matters. History: docs/history/compare.md.
fn draw_line_ending_overlay(frame: &mut Frame, area: Rect, line_endings: &[Option<LineEnding>], viewport_top_row: usize, theme: &Theme) {
    let inner = Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    for screen_row in 0..inner.height {
        let real_row = viewport_top_row + screen_row as usize;
        let Some(marker) = line_endings.get(real_row).copied().flatten().map(|ending| ending.marker().trim_start().to_string()) else { continue };
        let width = marker.chars().count() as u16;
        if width == 0 || width > inner.width {
            continue;
        }
        let marker_area = Rect { x: inner.x + inner.width - width, y: inner.y + screen_row, width, height: 1 };
        frame.render_widget(Paragraph::new(Line::from(Span::styled(marker, Style::default().fg(theme.text_dim)))), marker_area);
    }
}


#[cfg(test)]
mod tests;
