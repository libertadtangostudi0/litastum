use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
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

fn draw_results(frame: &mut Frame, area: Rect, state: &FindFileState, theme: &Theme) {
    let height = (state.results.len().max(1) as u16 + 6).clamp(8, area.height);
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
        frame.render_widget(List::new(items), rows[0]);
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
        Span::styled("Ctrl+S", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" export  ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[3]);
}
