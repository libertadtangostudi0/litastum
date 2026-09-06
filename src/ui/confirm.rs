use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::{PendingDelete, PendingTransfer, TransferOp};
use crate::text_field;
use crate::theming::Theme;
use crate::ui::centered_rect;

/// Renders the F8 "delete this?" prompt over the browser.
pub fn draw_confirm_delete_popup(frame: &mut Frame, area: Rect, pending: &PendingDelete, theme: &Theme) {
    let popup = centered_rect(50, 4, area);

    frame.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.danger))
        .title(" Delete ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let question = if pending.is_dir {
        format!("Delete directory '{}' and all its contents?", pending.name)
    } else {
        format!("Delete '{}'?", pending.name)
    };

    let lines = vec![
        Line::from(Span::styled(question, Style::default().fg(theme.text))),
        Line::from(vec![
            Span::styled("Y", Style::default().fg(theme.danger).add_modifier(Modifier::BOLD)),
            Span::styled(" delete    ", Style::default().fg(theme.text_dim)),
            Span::styled("N", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
            Span::styled(" / Esc cancel", Style::default().fg(theme.text_dim)),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Renders the F5/F6 "copy/move to?" prompt: source name and operation
/// on one line, an editable destination path on the next (defaults to
/// the other panel's directory — see `explorer::command::request_transfer`).
/// Returns where the real terminal cursor should sit, same mechanism as
/// the command line's own cursor (`ui::draw`).
pub fn draw_confirm_transfer_popup(frame: &mut Frame, area: Rect, pending: &PendingTransfer, theme: &Theme) -> Position {
    let popup = centered_rect(60, 6, area);

    frame.render_widget(Clear, popup);

    let verb = match pending.operation {
        TransferOp::Copy => "Copy",
        TransferOp::Move => "Move",
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .title(format!(" {verb} "));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    let source_line = Line::from(Span::styled(
        format!("{verb} '{}' to:", pending.name),
        Style::default().fg(theme.text),
    ));
    frame.render_widget(source_line, rows[0]);

    frame.render_widget(destination_line(pending, theme), rows[1]);

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" {verb}   "), Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[3]);

    Position {
        x: rows[1].x + pending.cursor as u16,
        y: rows[1].y,
    }
}

/// Renders the destination text with its `Shift+Left`/`Shift+Right`
/// selection (`text_field::selection_range`), if any, picked out with
/// the same highlight background used for the active row in a panel
/// (`theme.current_row_bg`) — no selection just renders as plain text.
fn destination_line(pending: &PendingTransfer, theme: &Theme) -> Line<'static> {
    let Some(anchor) = pending.selection_anchor else {
        return Line::from(Span::styled(pending.destination.clone(), Style::default().fg(theme.text)));
    };

    let (start, end) = text_field::selection_range(anchor, pending.cursor);
    let chars: Vec<char> = pending.destination.chars().collect();
    let before: String = chars[..start].iter().collect();
    let selected: String = chars[start..end].iter().collect();
    let after: String = chars[end..].iter().collect();

    Line::from(vec![
        Span::styled(before, Style::default().fg(theme.text)),
        Span::styled(selected, Style::default().fg(theme.text).bg(theme.current_row_bg)),
        Span::styled(after, Style::default().fg(theme.text)),
    ])
}
