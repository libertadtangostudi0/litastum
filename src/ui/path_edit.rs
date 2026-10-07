use ratatui::{
    layout::{Position, Rect},
    style::Style,
    text::Line,
    widgets::{Clear, Paragraph},
    Frame,
};

use crate::text_field::TextField;
use crate::theming::Theme;

/// The path field over a pane's top border (`PathEdit`): plain text, its
/// selection styled like the panels' selected row (`selection_text`, so
/// it stays readable on a bright selection color). Scrolls sideways to keep
/// the caret in view -- it starts at the end, by the file name. Returns
/// the caret's cell.
pub(super) fn draw_path_field(frame: &mut Frame, area: Rect, field: &TextField, theme: &Theme) -> Option<Position> {
    if area.width == 0 || area.height == 0 {
        return None;
    }
    let scroll = (field.cursor() + 1).saturating_sub(usize::from(area.width));
    let style = Style::default().fg(theme.text).bg(theme.bg);
    let line = Line::from(super::text_field::styled_field_spans(field, style, super::popup::selected_row_style(theme)));
    // A `Paragraph` leaves the cells past its text alone, so the title
    // under them showed through -- the caret cell at the end repeated the
    // file name's last character.
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(line).style(style).scroll((0, scroll as u16)), area);
    Some(Position::new(area.x + (field.cursor() - scroll) as u16, area.y))
}
