use edtui::{EditorState, EditorTheme, EditorView, Highlight, Index2, LineNumbers, Lines, RowIndex};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::Block,
    Frame,
};

use crate::compare::{ComparePane, CompareState, DiffLineKind, LineEndingDisplay};
use crate::theming::Theme;

/// Renders `Alt+F5`'s own full-screen comparer -- two files side by
/// side, a shared vertical scroll (`CompareState::scroll_row`), and a
/// one-line hint bar, mirroring `editor_pane.rs::draw_editor`'s own
/// "Min(3) content / Length(1) hint" vertical split.
///
/// **Read-only, and rebuilt from scratch every frame** -- unlike
/// `Editor::view`, which reuses one long-lived `edtui::EditorState`
/// across frames (there's real cursor/undo/selection state to keep),
/// each pane here builds a brand new `EditorState` on every single
/// call, straight from `ComparePane::lines`. This is deliberate, not
/// an oversight: nothing in `compare::input` ever forwards a key event
/// into either `EditorState` (there's no cursor to move, no buffer to
/// edit), so there's no state that would actually need to survive
/// between frames except the scroll position, which lives on
/// `CompareState` itself and gets reapplied
/// (`EditorState::set_viewport_offset`) every time regardless. Building
/// fresh also means the F9 -> Line endings toggle
/// (`App::compare_line_ending_display`) just works by construction --
/// the marker is baked into the very text handed to `Lines::from`,
/// nothing to invalidate or rebuild on a toggle beyond the next redraw
/// that was already about to happen anyway.
pub(super) fn draw_compare(frame: &mut Frame, area: Rect, state: &CompareState, theme: &Theme, line_ending_display: LineEndingDisplay) {
    let rows = Layout::default().direction(Direction::Vertical).constraints([Constraint::Min(3), Constraint::Length(1)]).split(area);
    let panes = Layout::default().direction(Direction::Horizontal).constraints([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[0]);

    draw_pane(frame, panes[0], &state.left, state.scroll_row, theme, line_ending_display, theme.diff_removed_bg);
    draw_pane(frame, panes[1], &state.right, state.scroll_row, theme, line_ending_display, theme.diff_added_bg);

    let hint = Line::from(vec![
        Span::styled("Up/Down ", Style::default().fg(theme.accent)),
        Span::styled("Scroll   ", Style::default().fg(theme.text_dim)),
        Span::styled("Tab/Shift+Tab ", Style::default().fg(theme.accent)),
        Span::styled("Next/prev diff   ", Style::default().fg(theme.text_dim)),
        Span::styled("F9 ", Style::default().fg(theme.accent)),
        Span::styled("Menu   ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc ", Style::default().fg(theme.accent)),
        Span::styled("Close", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[1]);
}

fn draw_pane(frame: &mut Frame, area: Rect, pane: &ComparePane, scroll_row: usize, theme: &Theme, line_ending_display: LineEndingDisplay, changed_bg: Color) {
    let text = pane_text(pane, line_ending_display);
    let mut edtui_state = EditorState::new(Lines::from(text.as_str()));
    // `set_viewport_offset` alone isn't enough -- `edtui`'s own render
    // pass (`EditorView::render`, `state/view.rs::update_viewport_vertical`)
    // recomputes the viewport from `state.cursor` on *every* render to
    // keep the cursor visible (documented directly on
    // `set_viewport_offset` itself: "the viewport may be adjusted
    // during the next render... depending on the cursor position"). A
    // fresh `EditorState` always starts with `cursor` at row 0, so
    // without this, `update_viewport_vertical` saw `cursor_row (0) <
    // viewport.y (scroll_row)` on every single frame and snapped the
    // offset straight back to 0 -- reported directly as "arrow-key
    // scroll doesn't work at all". Parking the (hidden, via
    // `.hide_cursor()`) cursor on the same row as the requested
    // viewport offset keeps `update_viewport_vertical`'s own scroll-up/
    // scroll-down checks both false, so it leaves the offset alone.
    edtui_state.cursor = Index2::new(scroll_row, 0);
    edtui_state.set_viewport_offset(0, scroll_row);
    edtui_state.highlights = line_highlights(pane, &edtui_state.lines, changed_bg, theme);

    let editor_theme = EditorTheme::default()
        .base(Style::default().fg(theme.text).bg(theme.bg))
        .block(Block::bordered().border_style(Style::default().fg(theme.accent)).title(pane.path.to_string_lossy().into_owned()))
        .hide_status_line()
        .hide_cursor()
        .line_numbers_style(Style::default().fg(theme.text_dim).bg(theme.bg));

    let view = EditorView::new(&mut edtui_state).theme(editor_theme).line_numbers(LineNumbers::Absolute);
    frame.render_widget(view, area);
}

/// The literal text fed into `Lines::from` for one pane -- `pane.lines`
/// as-is when line endings are hidden; with a per-line `" [CRLF]"`/
/// `" [LF]"` marker (`LineEnding::marker`) appended when shown, looked
/// up via `source_index` (an `Empty` padding row, `None`, gets no
/// marker -- there's no real line to have an ending at all).
fn pane_text(pane: &ComparePane, line_ending_display: LineEndingDisplay) -> String {
    if line_ending_display == LineEndingDisplay::Hidden {
        return pane.lines.join("\n");
    }
    pane.lines
        .iter()
        .enumerate()
        .map(|(row, line)| {
            let marker = pane.source_index[row].and_then(|source_row| pane.line_endings.get(source_row).copied().flatten()).map(|ending| ending.marker()).unwrap_or("");
            format!("{line}{marker}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// One whole-line `Highlight` per changed (`Removed`/`Added`) or
/// padding (`Empty`) row -- `Unchanged` rows get none at all, rendering
/// as plain, un-tinted text (this app's own file panel's ordinary
/// coloring, not a syntax-highlighted one -- see this file's own top
/// doc comment for why syntax highlighting isn't wired up here yet).
///
/// **A `Highlight`'s own style *replaces* whatever's under it
/// outright** -- confirmed directly from `edtui`'s own rendering
/// (`word_highlight.rs`'s own doc comment already established this for
/// word-occurrence highlighting): there's no way to tint just the
/// background while leaving per-token syntax coloring underneath, so a
/// changed line renders in one flat foreground/background pair, the
/// same tradeoff a text selection in the built-in editor already
/// accepts.
fn line_highlights(pane: &ComparePane, lines: &Lines, changed_bg: Color, theme: &Theme) -> Vec<Highlight> {
    let mut highlights = Vec::new();
    for (row, kind) in pane.kinds.iter().enumerate() {
        let style = match kind {
            DiffLineKind::Removed | DiffLineKind::Added => Style::default().fg(theme.text).bg(changed_bg),
            DiffLineKind::Empty => Style::default().fg(theme.text_dim).bg(theme.border),
            DiffLineKind::Unchanged => continue,
        };
        let Some(line) = lines.get(RowIndex::new(row)) else { continue };
        let end_col = line.len().saturating_sub(1);
        highlights.push(Highlight::new(Index2::new(row, 0), Index2::new(row, end_col), style));
    }
    highlights
}


#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;
    use crate::compare::CompareState;
    use crate::test_support::unique_scratch_dir;

    fn open_pair(left_content: &str, right_content: &str) -> CompareState {
        let dir = unique_scratch_dir("ui-compare");
        let left_path = dir.join("left.txt");
        let right_path = dir.join("right.txt");
        std::fs::write(&left_path, left_content).unwrap();
        std::fs::write(&right_path, right_content).unwrap();
        CompareState::open(left_path, right_path).unwrap()
    }

    fn rendered(state: &CompareState, theme: &Theme) -> String {
        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                draw_compare(frame, frame.area(), state, theme, LineEndingDisplay::Hidden);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Regression test for a real report: pressing Up/Down did nothing
    /// visible at all. `edtui`'s own render pass recomputes the
    /// viewport from `state.cursor` on every frame
    /// (`update_viewport_vertical`) to keep the cursor visible -- a
    /// fresh, per-frame `EditorState` always starts with `cursor` at
    /// row 0, so without `draw_pane`'s own `edtui_state.cursor =
    /// Index2::new(scroll_row, 0)` fix, the viewport snapped straight
    /// back to the top on every single render, no matter what
    /// `CompareState::scroll_row` said. A rendered-text assertion (not
    /// just checking `scroll_row` itself, which was always updating
    /// correctly -- the bug was purely in what got drawn) is the only
    /// way this catches a real regression here.
    #[test]
    fn scrolling_actually_moves_the_visible_window() {
        let lines: Vec<String> = (0..30).map(|i| format!("line{i}")).collect();
        let content = format!("{}\n", lines.join("\n"));
        let mut state = open_pair(&content, &content);
        let theme = Theme::dark();

        let before = rendered(&state, &theme);
        assert!(before.contains("line0"), "should start scrolled to the top");

        state.scroll_row = 20;
        let after = rendered(&state, &theme);
        assert!(!after.contains("line0"), "top of the viewport should have scrolled past line0");
        assert!(after.contains("line20"), "the row scrolled to should actually be visible");
    }
}
