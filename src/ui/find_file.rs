use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::explorer::{FindFilePhase, FindFileState};
use crate::theming::Theme;
use crate::ui::centered_rect;

/// Renders whichever phase is current. Returns where the real terminal
/// cursor should sit — only meaningful during `Typing` (same mechanism
/// as the command line's own cursor, `ui::draw`); `None` during
/// `Results`, which has no text entry to place a cursor in.
pub fn draw_find_file(frame: &mut Frame, area: Rect, state: &FindFileState, theme: &Theme) -> Option<Position> {
    match state.phase {
        FindFilePhase::Typing => Some(draw_typing(frame, area, state, theme)),
        FindFilePhase::Results => {
            draw_results(frame, area, state, theme);
            None
        }
    }
}

fn draw_typing(frame: &mut Frame, area: Rect, state: &FindFileState, theme: &Theme) -> Position {
    let popup = centered_rect(50, 5, area);
    frame.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .title(" Find file ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    let label = Line::from(Span::styled("File name to find:", Style::default().fg(theme.text)));
    frame.render_widget(label, rows[0]);

    let query = Line::from(Span::styled(state.query.clone(), Style::default().fg(theme.text)));
    frame.render_widget(query, rows[1]);

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" search  ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[2]);

    Position { x: rows[1].x + state.cursor as u16, y: rows[1].y }
}

/// Fixed popup height, independent of `state.results.len()` -- now that
/// the list actually scrolls (see the `ListState` below), there's no
/// reason for the popup itself to keep growing with the result count
/// the way it used to. Picked to match `ui/theme_menu.rs`'s own
/// color-scheme popup height directly, per an explicit side-by-side
/// comparison request -- that popup's own formula (`themes.len() + 13`)
/// comes out to `21` for the 8 themes bundled with this repo, so this
/// reuses that same number rather than inventing an unrelated one.
const RESULTS_HEIGHT: u16 = 21;

fn draw_results(frame: &mut Frame, area: Rect, state: &FindFileState, theme: &Theme) {
    let height = RESULTS_HEIGHT.min(area.height);
    let popup = centered_rect(70, height, area);
    frame.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .title(format!(" Find file: \"{}\" ", state.query));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    // Two fixed rows for the export message (label + detail, e.g.
    // "Exported to:" / the actual path) even when there isn't one --
    // simpler than resizing the popup depending on whether a message
    // is currently showing, at the cost of a little blank space in the
    // common "haven't exported yet" case.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    if state.results.is_empty() {
        let empty = Paragraph::new(Line::from(Span::styled("No matches found", Style::default().fg(theme.text_dim))));
        frame.render_widget(empty, rows[0]);
    } else {
        let items: Vec<ListItem> = state
            .results
            .iter()
            .enumerate()
            .map(|(index, path)| {
                let style = if index == state.selected {
                    Style::default().fg(theme.text).bg(theme.current_row_bg).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.text)
                };
                ListItem::new(Line::from(Span::styled(path.to_string_lossy().into_owned(), style)))
            })
            .collect();
        // A result count in the thousands (a big tree, a broad query)
        // used to render every item straight into this fixed-height
        // area with no scroll offset at all -- reported directly, the
        // same "List with no ListState doesn't auto-scroll" gap
        // `Panel`'s own entry grid hit earlier (see its own doc comment
        // in `explorer/panel/mod.rs`), and already fixed once in this
        // codebase for `ui/theme_menu.rs`'s picker the same way: a real
        // `ListState` tracking the selected index, which `List` then
        // scrolls to keep in view on its own.
        let list = List::new(items);
        let mut list_state = ListState::default().with_selected(Some(state.selected));
        frame.render_stateful_widget(list, rows[0], &mut list_state);
    }

    if let Some((label, detail)) = &state.export_message {
        let label_line = Line::from(Span::styled(label.clone(), Style::default().fg(theme.text_dim)));
        frame.render_widget(label_line, rows[1]);
        // A real Downloads path is easily wide enough to overflow the
        // popup on one line together with the label -- ratatui clips
        // rather than wraps a `Line` that's too long for its area, so
        // splitting the detail onto its own row (still just clipped if
        // it's *itself* wider than the popup, but that's a much rarer
        // case than "label + path together" was) is the fix here, not
        // a text-wrapping widget for what's meant to be a one-line
        // status.
        let detail_line = Line::from(Span::styled(detail.clone(), Style::default().fg(theme.text)));
        frame.render_widget(detail_line, rows[2]);
    }

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" go to  ", Style::default().fg(theme.text_dim)),
        Span::styled("Tab", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" peek  ", Style::default().fg(theme.text_dim)),
        Span::styled("F4", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" edit  ", Style::default().fg(theme.text_dim)),
        Span::styled("Ctrl+S", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" export  ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[3]);
}


#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ratatui::{backend::TestBackend, Terminal};

    use super::*;

    fn state_with(count: usize, selected: usize) -> FindFileState {
        FindFileState {
            phase: FindFilePhase::Results,
            query: "x".to_string(),
            cursor: 0,
            results: (0..count).map(|i| PathBuf::from(format!("C:/dev/file_{i:04}.txt"))).collect(),
            selected,
            export_message: None,
        }
    }

    fn rendered(state: &FindFileState) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        terminal.draw(|frame| {
            draw_find_file(frame, frame.area(), state, &theme);
        }).unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Regression test for the real report: a result count far past
    /// what the (terminal-height-clamped) popup can show at once used
    /// to render every item into a fixed-height list with no scroll
    /// offset at all -- the selected row, deep into a list of
    /// thousands, was simply invisible, scrolled off past the bottom of
    /// the rendered area with nothing to bring it into view. A real
    /// `ListState` (see the call site's own doc comment) should keep
    /// whichever row is selected actually on screen no matter how far
    /// into a long list it is.
    #[test]
    fn selecting_a_result_far_down_a_long_list_scrolls_it_into_view() {
        let state = state_with(2000, 1500);

        let text = rendered(&state);

        assert!(text.contains("file_1500"), "the selected result should be scrolled into view:\n{text}");
    }

    #[test]
    fn a_short_result_list_needs_no_scrolling_to_show_the_selection() {
        let state = state_with(3, 2);

        let text = rendered(&state);

        assert!(text.contains("file_0002"));
    }
}
