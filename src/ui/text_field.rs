use ratatui::style::Style;
use ratatui::text::{Line, Span};

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


/// `field_spans` as a whole line.
pub(super) fn field_line(field: &TextField, theme: &Theme) -> Line<'static> {
    Line::from(field_spans(field, theme))
}


#[cfg(test)]
mod tests {
    use super::*;

    fn texts(spans: &[Span]) -> Vec<String> {
        spans.iter().map(|span| span.content.to_string()).collect()
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
