use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::text_field::TextField;
use crate::theming::Theme;

use super::popup;


/// `field`'s text as spans, its selection (if any) in the selected-text
/// style every text field uses.
pub(super) fn field_spans(field: &TextField, theme: &Theme) -> Vec<Span<'static>> {
    styled_field_spans(field, Style::default().fg(theme.text), popup::selected_text_style(theme))
}


/// `field`'s text as spans in `text_style`, its selection in
/// `selected_style` -- for a field drawn on a background of its own.
pub(super) fn styled_field_spans(field: &TextField, text_style: Style, selected_style: Style) -> Vec<Span<'static>> {
    let Some((start, end)) = field.selection() else {
        return vec![Span::styled(field.text().to_string(), text_style)];
    };
    let chars: Vec<char> = field.text().chars().collect();
    vec![
        Span::styled(chars[..start].iter().collect::<String>(), text_style),
        Span::styled(chars[start..end].iter().collect::<String>(), selected_style),
        Span::styled(chars[end..].iter().collect::<String>(), text_style),
    ]
}


/// Draws `field` into its one-row `area`, scrolled sideways to keep the
/// caret in view, and returns the caret's cell. Reported: a text longer
/// than its field hid its end, the caret past the edge -- in the command
/// line, the `Ctrl+F` box, every popup's field.
pub(super) fn draw_field(frame: &mut Frame, area: Rect, field: &TextField, theme: &Theme) -> Position {
    let scroll = (field.cursor() + 1).saturating_sub(usize::from(area.width));
    frame.render_widget(Paragraph::new(Line::from(field_spans(field, theme))).scroll((0, scroll as u16)), area);
    Position { x: area.x + (field.cursor() - scroll) as u16, y: area.y }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn texts(spans: &[Span]) -> Vec<String> {
        spans.iter().map(|span| span.content.to_string()).collect()
    }

    fn drawn(field: &TextField, width: u16) -> (String, u16) {
        let mut cursor = Position::default();
        let text = crate::test_support::render_text(width, 1, |frame| cursor = draw_field(frame, frame.area(), field, &Theme::dark()));
        (text.lines().next().unwrap_or("").trim_end().to_string(), cursor.x)
    }

    /// Reported: a text longer than its field hid its end, the caret past
    /// the edge. It scrolls to keep the caret -- at the end here -- in view.
    #[test]
    fn a_long_text_scrolls_to_keep_the_caret_in_view() {
        let mut field = TextField::new();
        field.set_text("0123456789abcdefghij");

        let (shown, cursor) = drawn(&field, 10);

        assert_eq!(shown, "bcdefghij", "the end shows, the caret's cell after it");
        assert_eq!(cursor, 9);
    }

    #[test]
    fn a_caret_back_at_the_start_shows_the_start() {
        let mut field = TextField::new();
        field.set_text("0123456789abcdefghij");
        field.select_all();
        field.apply_key(crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Home, crossterm::event::KeyModifiers::NONE));

        let (shown, cursor) = drawn(&field, 10);

        assert_eq!(shown, "0123456789");
        assert_eq!(cursor, 0);
    }

    #[test]
    fn no_selection_is_one_plain_span() {
        let field = TextField::with_text("abc");
        assert_eq!(texts(&field_spans(&field, &Theme::dark())), vec!["abc"]);
    }

    #[test]
    fn a_selection_splits_into_before_selected_after() {
        let field = TextField::at("héllo", 1, Some(3));
        let spans = field_spans(&field, &Theme::dark());
        assert_eq!(texts(&spans), vec!["h", "él", "lo"], "split by characters, not bytes");
        assert_eq!(spans[1].style, popup::selected_text_style(&Theme::dark()));
    }
}
