use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem},
    Frame,
};

use crate::theming::{PopupStyle, PopupStyleMenu, Theme};
use crate::ui::popup;

/// Renders the F9 -> Options -> UI popup-style picker -- a plain list
/// of `PopupStyle::all()`, same shape as `ui/shell.rs`'s profile
/// picker. Drawn using `style` itself (the style still active *while*
/// picking, before `Enter` commits a new one) -- same as every other
/// popup in this app always rendering with whatever's currently active.
pub fn draw_popup_style_menu(frame: &mut Frame, area: Rect, menu: &PopupStyleMenu, theme: &Theme, style: PopupStyle) {
    let styles = PopupStyle::all();
    let extra = popup::chrome_extra_rows(style);
    let height = (styles.len() as u16 + 4 + extra).clamp(6 + extra, area.height);
    let inner = popup::draw_frame(frame, area, theme, style, Line::from(Span::raw(" UI ")), 30, height);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let items: Vec<ListItem> = styles
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            let label = if *candidate == style {
                format!("{} (current)", candidate.label())
            } else {
                candidate.label().to_string()
            };
            let item_style = if index == menu.selected {
                popup::selected_row_style(theme)
            } else {
                Style::default().fg(theme.text)
            };
            ListItem::new(Line::from(Span::styled(label, item_style)))
        })
        .collect();
    frame.render_widget(List::new(items), rows[0]);

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" apply  ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[1]);
}


#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;

    fn rendered(menu: &PopupStyleMenu, style: PopupStyle) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        terminal
            .draw(|frame| {
                draw_popup_style_menu(frame, frame.area(), menu, &theme, style);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn lists_both_styles_and_marks_the_current_one() {
        let menu = PopupStyleMenu::open(PopupStyle::Rounded);
        let text = rendered(&menu, PopupStyle::Rounded);
        assert!(text.contains("Classic"));
        assert!(text.contains("Rounded (current)"));
    }

    #[test]
    fn renders_fine_in_classic_style_too() {
        let menu = PopupStyleMenu::open(PopupStyle::Classic);
        let text = rendered(&menu, PopupStyle::Classic);
        assert!(text.contains("Classic (current)"));
        assert!(text.contains("Rounded"));
    }
}
