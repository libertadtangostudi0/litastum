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
    let scroll = view_scroll(field, usize::from(area.width));
    frame.render_widget(Paragraph::new(Line::from(field_spans(field, theme))).scroll((0, scroll as u16)), area);
    Position { x: area.x + (field.cursor() - scroll) as u16, y: area.y }
}


/// Where `field`'s view starts in a `width`-wide row (`scroll_for`),
/// noted on the field for the next frame -- for fields drawn by hand
/// (the `Ctrl+F` box, a path field).
pub(super) fn view_scroll(field: &TextField, width: usize) -> usize {
    let scroll = scroll_for(field, width);
    field.set_view_offset(scroll);
    scroll
}


/// Characters of context kept left of the caret while it moves left.
const MARGIN: usize = 4;


/// Where the view starts: where it was, moved only as much as the caret
/// needs -- with `MARGIN` characters before it -- and, when a selection
/// fits, so both its ends show. Reported: selecting leftwards from the
/// end, the view jumped to the start and the selected part went off the
/// right edge. Never past the end: the caret's cell after the text is the
/// last one.
fn scroll_for(field: &TextField, width: usize) -> usize {
    if width == 0 {
        return 0;
    }
    let cursor = field.cursor();
    let len = field.text().chars().count();
    let margin = MARGIN.min(width / 4);
    let mut scroll = field.view_offset();
    if cursor < scroll + margin {
        scroll = cursor.saturating_sub(margin);
    }
    if cursor + 1 > scroll + width {
        scroll = cursor + 1 - width;
    }
    if let Some((start, end)) = field.selection() {
        // `end` is past the last selected character; it stays in view too.
        if end - start < width {
            scroll = scroll.clamp((end + 1).saturating_sub(width), start);
        }
    }
    scroll.min((len + 1).saturating_sub(width))
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

    /// Reported: selecting leftwards from the end, the view jumped to the
    /// start and the selection's end went off the right edge. The view
    /// stays put while the caret is in it, so the selected part shows.
    #[test]
    fn selecting_leftwards_from_the_end_keeps_the_selection_in_view() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut field = TextField::new();
        field.set_text("0123456789abcdefghij");
        drawn(&field, 10);
        assert_eq!(field.view_offset(), 11, "the end shows");

        for _ in 0..5 {
            field.apply_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));
        }
        let (shown, cursor) = drawn(&field, 10);

        assert_eq!(field.selection(), Some((15, 20)));
        assert_eq!(shown, "bcdefghij", "the view didn't move: the selected 'fghij' shows");
        assert_eq!(cursor, 4);
    }

    /// Past the margin the view follows the caret left, keeping a few
    /// characters before it.
    #[test]
    fn moving_left_keeps_context_before_the_caret() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut field = TextField::new();
        field.set_text("0123456789abcdefghijklmnopqrstuvwxyz");
        drawn(&field, 20);
        for _ in 0..20 {
            field.apply_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        }

        let (shown, cursor) = drawn(&field, 20);

        assert_eq!(field.cursor(), 16);
        assert_eq!(cursor, 4, "four characters of context before the caret");
        assert!(shown.starts_with("cdef"), "{shown:?}");
    }

    /// A selection wider than the field can't show whole: the caret wins.
    #[test]
    fn a_selection_wider_than_the_field_follows_the_caret() {
        let mut field = TextField::new();
        field.set_text("0123456789abcdefghij");
        field.select_all();

        let (_, cursor) = drawn(&field, 10);

        assert!(cursor < 10);
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
