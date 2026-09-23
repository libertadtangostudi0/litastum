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

/// Renders `Alt+F5`'s own full-screen comparer: two ordinary, fully
/// editable `editor::Editor` panes side by side (real cursor, undo,
/// syntax highlighting, save -- everything `F4` editing already has),
/// with GitHub-style red/green diff backgrounds layered on top
/// (`Editor::set_extra_highlights`) and a one-line hint bar, mirroring
/// `editor_pane.rs::draw_editor`'s own "Min(3) content / Length(1)
/// hint" vertical split.
///
/// The diff itself is recomputed fresh every single frame, straight
/// from both panes' *live* text (`Editor::text`) -- there is no
/// snapshot taken at `CompareState::open` time that could drift from
/// what's actually being edited. Neither pane's real buffer is ever
/// touched by this: unlike the phase-1 read-only version this replaced,
/// no synthetic filler rows are inserted anywhere -- `compute`'s own
/// row-aligned `Empty` padding rows exist purely to classify real rows
/// and to drive `map_real_row` below, never to render as text.
///
/// Only the currently *focused* pane (`CompareState::focus`) scrolls
/// under its own steam (`Editor::view`'s usual cursor-follow behavior,
/// completely unmodified). The *other* pane's viewport is forced, every
/// frame, to whatever real row `map_real_row` says corresponds to the
/// focused pane's own current top row -- `Editor::set_viewport_top_row`,
/// the same technique (and the same underlying `edtui` viewport-follows-
/// cursor fact) already fixed a real scroll bug in the read-only
/// version of this view; see that method's own doc comment.
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
        Span::styled("Ctrl+Up/Down ", Style::default().fg(theme.accent)),
        Span::styled("Next/prev diff   ", Style::default().fg(theme.text_dim)),
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

/// One whole-line `Highlight` per changed (`Removed`/`Added`) real row
/// -- an `Empty` (padding, no real row on this side) or `Unchanged` row
/// gets none at all, rendering with `Editor::view`'s own ordinary
/// syntax-highlighted styling.
///
/// **A `Highlight`'s own style *replaces* whatever's under it
/// outright** -- confirmed directly from `edtui`'s own rendering
/// (`word_highlight.rs`'s own doc comment already established this for
/// word-occurrence highlighting): there's no way to tint just the
/// background while leaving per-token syntax coloring underneath, so a
/// changed line renders in one flat foreground/background pair, the
/// same tradeoff an active text selection in the built-in editor
/// already accepts.
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

/// Small right-aligned `[CRLF]`/`[LF]` markers over a pane's own
/// visible rows, one per real line currently on screen -- `F9` -> Line
/// endings' `Shown` setting. Drawn as a thin overlay *on top of* the
/// already-rendered `EditorView` rather than baked into the buffer text
/// the way the read-only phase-1 version did it: these panes are real,
/// editable, saved-to-disk buffers now, and inserting extra characters
/// into them to show a marker would corrupt the file the moment it's
/// saved. `line_endings` is `CompareState`'s own fixed on-open snapshot
/// (`CompareState::line_endings`'s own doc comment explains why it
/// can't be redetected from the live buffer at all -- `edtui` itself
/// throws the `\r` away on load). `inner` approximates `Editor::view`'s
/// own bordered content area (`Block::bordered()` is always a uniform
/// 1-cell frame) -- there's no direct hook into its internal layout to
/// read this back exactly, but the approximation only has to be right
/// along the right edge, which line numbers (drawn on the *left*)
/// never affect.
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

    /// Regression coverage: requested directly, syntax highlighting
    /// competed for attention with the diff coloring, so
    /// `CompareState::open` now calls `Editor::disable_syntax_highlighting`
    /// on both panes. `.rs` content (which `syntect`'s bundled default
    /// grammar set genuinely recognizes) is deliberately used here, not
    /// `.txt` -- a `.txt` file would never get a syntax highlighter in
    /// the first place, so it couldn't tell "disabled" apart from
    /// "nothing recognized this file" at all.
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

    /// Regression coverage for the same class of bug the read-only
    /// phase-1 version had (`ui/compare.rs`'s own git history): the
    /// *unfocused* pane's viewport must actually follow the focused
    /// one every frame, not just report the right `viewport_top_row()`
    /// internally while rendering something stale. Scrolls the focused
    /// (left) pane down past an inserted line and checks the *rendered
    /// text* of the unfocused (right) pane landed on the diff-mapped
    /// row, not row 0.
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
