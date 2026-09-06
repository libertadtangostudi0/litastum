use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem},
    Frame,
};

use crate::theming::{MainMenu, MenuLevel, Theme};
use crate::ui::centered_rect;

/// Renders the F9 top menu: whichever level's items are current
/// (`MenuLevel::items`), with the highlighted row picked out.
pub fn draw_main_menu(frame: &mut Frame, area: Rect, menu: &MainMenu, theme: &Theme) {
    let items = menu.level.items();
    let height = (items.len() as u16 + 4).clamp(6, area.height);
    let popup = centered_rect(30, height, area);

    frame.render_widget(Clear, popup);

    let title = match menu.level {
        MenuLevel::Main => " Menu ",
        MenuLevel::Commands => " Commands ",
        MenuLevel::Options => " Options ",
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .title(title);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let style = if index == menu.selected {
                Style::default().fg(theme.text).bg(theme.current_row_bg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text)
            };
            ListItem::new(Line::from(Span::styled(*label, style)))
        })
        .collect();
    frame.render_widget(List::new(list_items), rows[0]);

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" open  ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" back", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[1]);
}
