use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{List, ListItem, ListState},
    Frame,
};

use crate::explorer::TrustedLinksMenu;
use crate::theming::{PopupStyle, Theme};
use crate::ui::popup;

/// F9 -> Options -> Trusted links: the list (scrolled to the highlight),
/// then `del forget` / `esc close`.
pub fn draw_trusted_links_menu(frame: &mut Frame, area: Rect, menu: &TrustedLinksMenu, theme: &Theme, style: PopupStyle) {
    let width = area.width.saturating_sub(4).min(80);
    let extra = popup::chrome_extra_rows(style);
    // The list, the separator and hints, the two border rows.
    let height = (menu.links().len().max(1) as u16 + 2 + 2 + extra).clamp(5 + extra, area.height);
    let inner = popup::draw_frame(frame, area, theme, style, Line::from(Span::raw(" Trusted links ")), width, height);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    if menu.links().is_empty() {
        let empty = "No trusted links: a link asks before it opens, T on it trusts it";
        frame.render_widget(Line::from(Span::styled(empty, Style::default().fg(theme.text_dim))), rows[0]);
    } else {
        let items: Vec<ListItem> = menu
            .links()
            .iter()
            .enumerate()
            .map(|(index, link)| {
                let item_style = if index == menu.selected() { popup::selected_row_style(theme) } else { Style::default().fg(theme.text) };
                ListItem::new(Line::from(Span::styled(link.clone(), item_style)))
            })
            .collect();
        let mut state = ListState::default().with_selected(Some(menu.selected()));
        frame.render_stateful_widget(List::new(items), rows[0], &mut state);
    }

    frame.render_widget(popup::separator(inner.width, theme), rows[1]);
    let hints = Line::from(vec![popup::key_pill("del", "forget", theme.danger, theme), Span::raw("  "), popup::key_pill("esc", "close", theme.accent, theme)]);
    frame.render_widget(hints, rows[2]);
}


#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(menu: &TrustedLinksMenu, style: PopupStyle) -> String {
        let theme = Theme::dark();
        crate::test_support::render_text(100, 24, |frame| draw_trusted_links_menu(frame, frame.area(), menu, &theme, style))
    }

    #[test]
    fn shows_every_trusted_link_and_the_hints() {
        crate::explorer::save_trusted_for_tests(&["https://example.com/a".to_string(), r"C:\docs\b.md".to_string()]);
        let menu = TrustedLinksMenu::open();
        for style in [PopupStyle::Rounded, PopupStyle::Classic] {
            let text = rendered(&menu, style);
            assert!(text.contains("Trusted links"), "{style:?}");
            assert!(text.contains("https://example.com/a"), "{style:?}");
            assert!(text.contains(r"C:\docs\b.md"), "{style:?}");
            assert!(text.contains("forget"), "{style:?}");
        }
    }

    #[test]
    fn an_empty_list_says_how_a_link_gets_on_it() {
        let menu = TrustedLinksMenu::open();
        assert!(rendered(&menu, PopupStyle::Rounded).contains("No trusted links"));
    }

    /// 200 links don't fit: the list scrolls to the highlight.
    #[test]
    fn a_long_list_scrolls_to_the_highlight() {
        let links: Vec<String> = (0..100).map(|i| format!("https://example.com/{i:03}")).collect();
        crate::explorer::save_trusted_for_tests(&links);
        let mut menu = TrustedLinksMenu::open();
        for _ in 0..99 {
            menu.move_down();
        }
        assert!(rendered(&menu, PopupStyle::Rounded).contains("https://example.com/000"), "the oldest, last in the list");
    }
}
