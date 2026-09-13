use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem},
    Frame,
};

use crate::theming::{MainMenu, MenuLevel, PopupStyle, Theme};
use crate::ui::popup;

/// Renders the F9 top menu: whichever level's items are current
/// (`MenuLevel::items`), with the highlighted row picked out.
pub fn draw_main_menu(frame: &mut Frame, area: Rect, menu: &MainMenu, theme: &Theme, style: PopupStyle) {
    let items = menu.level.items();
    let extra = popup::chrome_extra_rows(style);
    let height = (items.len() as u16 + 4 + extra).clamp(6 + extra, area.height);

    let title = match menu.level {
        MenuLevel::Main => " Menu ",
        MenuLevel::Commands => " Commands ",
        MenuLevel::Options => " Options ",
    };
    let inner = popup::draw_frame(frame, area, theme, style, Line::from(Span::raw(title)), 30, height);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let style = if index == menu.selected {
                popup::selected_row_style(theme)
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
