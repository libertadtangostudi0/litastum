use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use crate::theming::{Theme, ThemeMenu};
use crate::ui::centered_rect;

/// Renders the F9 color-scheme picker popup: a list of theme names
/// found in the config dir, or a hint that none were found.
pub fn draw_theme_menu(frame: &mut Frame, area: Rect, menu: &ThemeMenu, theme: &Theme) {
    let height = (menu.themes.len().max(1) as u16 + 4).clamp(6, area.height);
    let popup = centered_rect(46, height, area);

    frame.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .title(" Color scheme ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    if menu.themes.is_empty() {
        let empty = Paragraph::new(Line::from(Span::styled(
            "No themes found — drop a Windows Terminal scheme .json",
            Style::default().fg(theme.text_dim),
        )));
        frame.render_widget(empty, rows[0]);
    } else {
        let items: Vec<ListItem> = menu
            .themes
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let style = if index == menu.selected {
                    Style::default().fg(theme.text).bg(theme.current_row_bg).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.text)
                };
                ListItem::new(Line::from(Span::styled(name.clone(), style)))
            })
            .collect();
        frame.render_widget(List::new(items), rows[0]);
    }

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" apply both  ", Style::default().fg(theme.text_dim)),
        Span::styled("I", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled("nterface  ", Style::default().fg(theme.text_dim)),
        Span::styled("E", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled("ditor  ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[1]);
}
