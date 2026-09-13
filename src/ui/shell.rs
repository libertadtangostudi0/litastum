use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem},
    Frame,
};

use crate::app::ShellMenu;
use crate::command_line::ShellProfile;
use crate::theming::{PopupStyle, Theme};
use crate::ui::popup;

/// Renders the `Ctrl+P` shell-profile picker popup.
pub fn draw_shell_menu(frame: &mut Frame, area: Rect, menu: &ShellMenu, profiles: &[ShellProfile], theme: &Theme, style: PopupStyle) {
    let extra = popup::chrome_extra_rows(style);
    let height = (profiles.len() as u16 + 4 + extra).clamp(6 + extra, area.height);
    let inner = popup::draw_frame(frame, area, theme, style, Line::from(Span::raw(" Shell ")), 36, height);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let items: Vec<ListItem> = profiles
        .iter()
        .enumerate()
        .map(|(index, profile)| {
            let style = if index == menu.selected {
                popup::selected_row_style(theme)
            } else {
                Style::default().fg(theme.text)
            };
            ListItem::new(Line::from(Span::styled(profile.name.clone(), style)))
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
