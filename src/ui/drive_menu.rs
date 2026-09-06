use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem},
    Frame,
};

use crate::explorer::{format_bytes, DriveMenu};
use crate::theming::Theme;
use crate::ui::centered_rect;

/// Renders the `Alt+F1`/`Alt+F2` "change drive" popup: one row per
/// drive (letter, type, total/free space — `"—"` for a size that
/// couldn't be read, e.g. an empty removable/CD drive).
pub fn draw_drive_menu(frame: &mut Frame, area: Rect, menu: &DriveMenu, theme: &Theme) {
    let height = (menu.drives.len().max(1) as u16 + 4).clamp(6, area.height);
    let popup = centered_rect(50, height, area);

    frame.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .title(" Change drive ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let items: Vec<ListItem> = menu
        .drives
        .iter()
        .enumerate()
        .map(|(index, drive)| {
            let total = drive.total_bytes.map_or_else(|| "—".to_string(), format_bytes);
            let free = drive.free_bytes.map_or_else(|| "—".to_string(), format_bytes);
            let text = format!("{:<4}{:<10}{total:>10}{free:>10}", drive.label, drive.kind);
            let style = if index == menu.selected {
                Style::default().fg(theme.text).bg(theme.current_row_bg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text)
            };
            ListItem::new(Line::from(Span::styled(text, style)))
        })
        .collect();
    frame.render_widget(List::new(items), rows[0]);

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" select  ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[1]);
}
