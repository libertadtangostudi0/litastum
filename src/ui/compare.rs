use edtui::{Highlight, Index2};
use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::compare::{compute, map_real_row, CompareState, DiffLineKind, LineEnding, LineEndingDisplay, Side};
use crate::editor::Editor;
use crate::theming::Theme;

/// `Alt+F5`: two ordinary, editable `Editor` panes side by side, with
/// red/green diff backgrounds (`Editor::set_extra_highlights`) and a hint
/// row. The diff is recomputed every frame from the live text; its
/// `Empty` padding rows are never drawn. Only the focused pane scrolls
/// itself -- the other is aligned every frame via `map_real_row` and
/// `Editor::set_viewport_top_row`. History: docs/history/compare.md.
pub(super) fn draw_compare(frame: &mut Frame, area: Rect, state: &mut CompareState, theme: &Theme, line_ending_display: LineEndingDisplay) -> Option<Position> {
    let rows = Layout::default().direction(Direction::Vertical).constraints([Constraint::Min(3), Constraint::Length(1)]).split(area);
    let panes = Layout::default().direction(Direction::Horizontal).constraints([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[0]);

    let left_text = state.left.text();
    let right_text = state.right.text();
    let (left_diff, right_diff) = compute(&left_text, &right_text);

    state.left.set_extra_highlights(row_highlights(&left_diff.kinds, &left_diff.source_index, &left_text, theme.diff_removed_bg, theme));
    state.right.set_extra_highlights(row_highlights(&right_diff.kinds, &right_diff.source_index, &right_text, theme.diff_added_bg, theme));

    match state.focus {
        Side::Left => {
            let target = map_real_row(&left_diff.source_index, &right_diff.source_index, state.left.viewport_top_row());
            state.right.set_viewport_top_row(target);
        }
        Side::Right => {
            let target = map_real_row(&right_diff.source_index, &left_diff.source_index, state.right.viewport_top_row());
            state.left.set_viewport_top_row(target);
        }
    }

    let left_line_endings = state.line_endings(Side::Left).to_vec();
    let right_line_endings = state.line_endings(Side::Right).to_vec();
    let left_cursor = draw_pane(frame, panes[0], &mut state.left, theme, state.focus == Side::Left, line_ending_display, &left_line_endings);
    let right_cursor = draw_pane(frame, panes[1], &mut state.right, theme, state.focus == Side::Right, line_ending_display, &right_line_endings);

    let hint = Line::from(vec![
        Span::styled("Tab ", Style::default().fg(theme.accent)),
        Span::styled("Switch pane   ", Style::default().fg(theme.text_dim)),
        Span::styled("F7/F8 ", Style::default().fg(theme.accent)),
        Span::styled("Prev/next diff   ", Style::default().fg(theme.text_dim)),
        Span::styled("Ctrl+S ", Style::default().fg(theme.accent)),
        Span::styled("Save   ", Style::default().fg(theme.text_dim)),
        Span::styled("F9 ", Style::default().fg(theme.accent)),
        Span::styled("Menu   ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc ", Style::default().fg(theme.accent)),
        Span::styled("Close", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[1]);
    left_cursor.or(right_cursor)
}

fn draw_pane(frame: &mut Frame, area: Rect, editor: &mut Editor, theme: &Theme, is_focused: bool, line_ending_display: LineEndingDisplay, line_endings: &[Option<LineEnding>]) -> Option<Position> {
    let viewport_top_row = editor.viewport_top_row();
    frame.render_widget(editor.view(theme, area), area);
    let cursor = if is_focused { editor.cursor_screen_position() } else { None };
    if line_ending_display == LineEndingDisplay::Shown {
        draw_line_ending_overlay(frame, area, line_endings, viewport_top_row, theme);
    }
    cursor
}

/// One whole-line `Highlight` per `Removed`/`Added` row. A `Highlight`
/// replaces the span's style, so a changed line is one flat color pair.
fn row_highlights(kinds: &[DiffLineKind], source_index: &[Option<usize>], text: &str, changed_bg: Color, theme: &Theme) -> Vec<Highlight> {
    let real_lines: Vec<&str> = text.lines().collect();
    let style = Style::default().fg(theme.text).bg(changed_bg);
    let mut highlights = Vec::new();
    for (row, kind) in kinds.iter().enumerate() {
        if !matches!(kind, DiffLineKind::Removed | DiffLineKind::Added) {
            continue;
        }
        let Some(real_row) = source_index[row] else { continue };
        let Some(line) = real_lines.get(real_row) else { continue };
        let end_col = line.chars().count().saturating_sub(1);
        highlights.push(Highlight::new(Index2::new(real_row, 0), Index2::new(real_row, end_col), style));
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
mod tests {
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;
    use crate::editor::EditorKeymapMode;
    use crate::test_support::unique_scratch_dir;

    fn open_pair(left_content: &str, right_content: &str) -> CompareState {
        let dir = unique_scratch_dir("ui-compare");
        let left_path = dir.join("left.txt");
        let right_path = dir.join("right.txt");
        std::fs::write(&left_path, left_content).unwrap();
        std::fs::write(&right_path, right_content).unwrap();
        CompareState::open(left_path, right_path, None, EditorKeymapMode::Standard).unwrap()
    }

    /// Compare panes have no syntax colors (they competed with the diff). Uses
    /// `.rs`, which `syntect` recognizes -- `.txt` couldn't tell "disabled"
    /// from "unrecognized".
    #[test]
    fn syntax_highlighting_is_disabled_even_for_a_recognized_language() {
        let dir = crate::test_support::unique_scratch_dir("ui-compare-no-syntax");
        let left_path = dir.join("left.rs");
        let right_path = dir.join("right.rs");
        let content = "fn main() {\n    // a comment\n    let x = 1;\n}\n";
        std::fs::write(&left_path, content).unwrap();
        std::fs::write(&right_path, content).unwrap();
        let mut state = CompareState::open(left_path, right_path, None, EditorKeymapMode::Standard).unwrap();
        let theme = Theme::dark();

        let buffer = render(&mut state, &theme, LineEndingDisplay::Hidden);

        // Every rendered cell in the left pane's content area (past the
        // border and line-number gutter) should be plain `theme.text` --
        // a real syntax highlighter would color "fn"/the comment/the
        // numeric literal distinctly from this.
        for y in 1..6u16 {
            for x in 4..28u16 {
                let cell = &buffer[(x, y)];
                if cell.symbol() != " " {
                    assert_eq!(cell.fg, theme.text, "cell ({x},{y}) = {:?} should be plain theme.text, not syntax-colored", cell.symbol());
                }
            }
        }
    }

    fn render(state: &mut CompareState, theme: &Theme, line_ending_display: LineEndingDisplay) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(60, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| { draw_compare(frame, frame.area(), state, theme, line_ending_display); }).unwrap();
        terminal.backend().buffer().clone()
    }

    /// A changed row should carry the diff background on both panes;
    /// an unchanged row should carry neither. Checks across each row's
    /// full x range rather than one hand-picked column, since a
    /// `Highlight` only ever covers a line's own real character span --
    /// not the empty space past it -- and the exact column that lands
    /// on depends on the line-number gutter's own width.
    #[test]
    fn changed_lines_get_a_diff_colored_background() {
        let mut state = open_pair("aaaa\nbbbb\ncccc\n", "aaaa\nxxxx\ncccc\n");
        let theme = Theme::dark();
        let buffer = render(&mut state, &theme, LineEndingDisplay::Hidden);

        let row_has_bg = |buffer: &ratatui::buffer::Buffer, x_range: std::ops::Range<u16>, y: u16, bg: ratatui::style::Color| x_range.clone().any(|x| buffer[(x, y)].bg == bg);

        // Row 0 ("aaaa", unchanged) is at screen y = 1 (past the
        // border); row 1 ("bbbb"/"xxxx", changed) is at y = 2.
        assert!(!row_has_bg(&buffer, 0..30, 1, theme.diff_removed_bg), "unchanged row 0 shouldn't carry a diff background");
        assert!(row_has_bg(&buffer, 0..30, 2, theme.diff_removed_bg), "left's changed row should be red somewhere");
        assert!(row_has_bg(&buffer, 30..60, 2, theme.diff_added_bg), "right's changed row should be green somewhere");
    }

    /// The unfocused pane really renders at the diff-mapped row after the
    /// focused one scrolls -- checked on the rendered text, not just
    /// `viewport_top_row()`.
    #[test]
    fn the_unfocused_panes_viewport_tracks_the_focused_one() {
        let left = "a\nb\nc\nd\ne\nf\ng\nh\n";
        let right = "a\nnew\nb\nc\nd\ne\nf\ng\nh\n";
        let mut state = open_pair(left, right);
        let theme = Theme::dark();

        // Scroll the focused (left) pane so real row 3 ("d") is at the
        // top of its viewport.
        state.left.set_viewport_top_row(3);
        let buffer = render(&mut state, &theme, LineEndingDisplay::Hidden);

        let right_top_row_text: String = (30..60).map(|x| buffer[(x, 1)].symbol().to_string()).collect();
        assert!(right_top_row_text.contains('d'), "right's own top row should have followed left's scroll to \"d\", not stayed at \"a\" (row 0)");
    }

    /// `F8` to a hunk below the screen centers it, like a `Ctrl+F` match.
    /// The 12-row area leaves an 11-row pane above the hint row: 9 content
    /// rows, so row 40 lands 4 rows down. History: docs/history/compare.md.
    #[test]
    fn a_hunk_jump_off_screen_is_centered() {
        let left: String = (0..60).map(|row| format!("line {row}\n")).collect();
        let right = left.replace("line 40\n", "changed\n");
        let mut state = open_pair(&left, &right);
        let theme = Theme::dark();
        render(&mut state, &theme, LineEndingDisplay::Hidden);

        state.jump_to_next_hunk();
        render(&mut state, &theme, LineEndingDisplay::Hidden);

        assert_eq!(state.left.cursor().row, 40);
        assert_eq!(state.left.viewport_top_row(), 36);
    }

    /// With long lines wrapped in the narrow panes, the hunk still lands in
    /// the middle -- counted in screen rows, not buffer rows -- and both
    /// panes show the same rows. Before, the wrapped rows above pushed it
    /// below the middle and `edtui`'s own re-scroll left the other pane a
    /// row off.
    #[test]
    fn a_hunk_jump_is_centered_in_screen_rows_when_lines_wrap() {
        // Every third line is 78 characters: 4 screen rows in a 25-column
        // text area (28 inside the border, minus a 3-column gutter).
        let left: String = (0..60).map(|row| format!("line {row} {}\n", "x".repeat(if row % 3 == 0 { 70 } else { 0 }))).collect();
        let right = left.replacen("line 40 ", "CHANGED ", 1);
        let mut state = open_pair(&left, &right);
        let theme = Theme::dark();
        render(&mut state, &theme, LineEndingDisplay::Hidden);

        state.jump_to_next_hunk();
        let buffer = render(&mut state, &theme, LineEndingDisplay::Hidden);

        // Row 39 wraps to 4 screen rows, so the view starts there and the
        // hunk (row 40) is on screen row 5 of 9 -- y = 5 past the border.
        assert_eq!(state.left.viewport_top_row(), 39);
        let row_text = |x_range: std::ops::Range<u16>| x_range.map(|x| buffer[(x, 5)].symbol().to_string()).collect::<String>();
        assert!(row_text(0..30).contains("line 40"), "{}", row_text(0..30));
        assert!(row_text(30..60).contains("CHANGED"), "the other pane shows the same row: {}", row_text(30..60));
    }

    /// A hunk already on screen is centered too, not left where it was.
    #[test]
    fn a_visible_hunk_is_centered_as_well() {
        let left: String = (0..60).map(|row| format!("line {row}\n")).collect();
        let right = left.replace("line 20\n", "c20\n").replace("line 24\n", "c24\n");
        let mut state = open_pair(&left, &right);
        let theme = Theme::dark();
        render(&mut state, &theme, LineEndingDisplay::Hidden);
        state.jump_to_next_hunk();
        render(&mut state, &theme, LineEndingDisplay::Hidden);
        assert_eq!(state.left.viewport_top_row(), 16);

        state.jump_to_next_hunk();
        render(&mut state, &theme, LineEndingDisplay::Hidden);

        assert_eq!(state.left.cursor().row, 24, "row 24 was already on screen (rows 16-24)");
        assert_eq!(state.left.viewport_top_row(), 20);
    }

    fn left_click(column: u16, row: u16) -> crossterm::event::MouseEvent {
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column,
            row,
            modifiers: crossterm::event::KeyModifiers::NONE,
        }
    }

    /// A click in the unfocused pane focuses it and puts the caret where
    /// it was clicked. Panes are 30 columns wide with a 2-column gutter
    /// ("1 ") inside the border, so text starts at x = 3 (left) and x = 33
    /// (right); content row 2 is at y = 3.
    #[test]
    fn a_click_focuses_the_pane_and_places_the_caret() {
        let mut state = open_pair("aaaa\nbbbb\ncccc\n", "aaaa\nxxxx\ncccc\n");
        let theme = Theme::dark();
        render(&mut state, &theme, LineEndingDisplay::Hidden);
        assert_eq!(state.focus, crate::compare::Side::Left);

        state.mouse(left_click(35, 3));
        render(&mut state, &theme, LineEndingDisplay::Hidden);

        assert_eq!(state.focus, crate::compare::Side::Right);
        assert_eq!(state.right.cursor(), edtui::Index2::new(2, 2));

        state.mouse(left_click(4, 2));

        assert_eq!(state.focus, crate::compare::Side::Left);
        assert_eq!(state.left.cursor(), edtui::Index2::new(1, 1));
    }

    #[test]
    fn line_ending_markers_show_up_only_when_shown() {
        let mut state = open_pair("a\r\n", "a\n");
        let theme = Theme::dark();

        let hidden = render(&mut state, &theme, LineEndingDisplay::Hidden);
        let hidden_text: String = (0..60).map(|x| hidden[(x, 1)].symbol().to_string()).collect();
        assert!(!hidden_text.contains("CRLF"), "no marker at all while Hidden");

        let shown = render(&mut state, &theme, LineEndingDisplay::Shown);
        let shown_text: String = (0..60).map(|x| shown[(x, 1)].symbol().to_string()).collect();
        assert!(shown_text.contains("CRLF"), "left's own CRLF-terminated line should show its marker while Shown");
    }
}
