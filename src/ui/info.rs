use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    Frame,
};

use crate::theming::{PopupStyle, Theme};

use super::popup;

/// Renders `Mode::Info`'s one-line notification -- dismissed by any
/// key (`main.rs::handle_event`). Currently the only caller is
/// `explorer::user_menu::input::handle_confirm_port_far_menu_key`
/// telling the user where a declined `FarMenu.ini` got backed up to,
/// but the mode itself carries a plain `String` rather than anything
/// user-menu-specific, so this stays a general-purpose primitive.
pub(super) fn draw_info_popup(frame: &mut Frame, area: Rect, message: &str, theme: &Theme, style: PopupStyle) {
    let extra = popup::chrome_extra_rows(style);
    let width = (message.chars().count() as u16 + 6).clamp(30, area.width);
    let height = 4 + extra;
    let inner = popup::draw_frame(frame, area, theme, style, Line::from(Span::raw(" Info ")), width, height);

    let rows = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(1), Constraint::Length(1)]).split(inner);

    frame.render_widget(Line::from(Span::styled(message.to_string(), Style::default().fg(theme.text))), rows[0]);

    let hint = Line::from(vec![Span::styled("any key", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)), Span::styled(" ok", Style::default().fg(theme.text_dim))]);
    frame.render_widget(hint, rows[1]);
}
