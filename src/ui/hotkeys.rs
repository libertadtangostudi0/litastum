use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState},
    Frame,
};

use crate::hotkeys::HotkeysMenu;
use crate::theming::{PopupStyle, Theme};
use crate::ui::popup;

/// The key column's width; the longest key is `Ctrl+Shift+Right`.
const KEY_WIDTH: usize = 18;
/// The action column's width; the "when" column takes the rest.
const ACTION_WIDTH: usize = 30;


/// F9 -> Options -> Hotkeys: a header, the rows (scrolled to the
/// highlight), then the hints.
pub fn draw_hotkeys(frame: &mut Frame, area: Rect, menu: &HotkeysMenu, theme: &Theme, style: PopupStyle) {
    let width = area.width.saturating_sub(4).min(80);
    let extra = popup::chrome_extra_rows(style);
    // The header, the rows, the separator and hints, the two border rows.
    let height = (menu.rows().len() as u16 + 1 + 2 + 2 + extra).min(area.height);
    let inner = popup::draw_frame(frame, area, theme, style, Line::from(Span::raw(" Hotkeys: panels ")), width, height);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    let header = Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD);
    frame.render_widget(Line::from(Span::styled(format!("{:KEY_WIDTH$}{:ACTION_WIDTH$}When", "Key", "Action"), header)), rows[0]);

    let items: Vec<ListItem> = menu
        .rows()
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let selected = index == menu.selected();
            let base = if selected { popup::selected_row_style(theme) } else { Style::default().fg(theme.text) };
            let key_style = if selected { base } else { Style::default().fg(theme.accent) };
            let dim = if selected { base } else { Style::default().fg(theme.text_dim) };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:KEY_WIDTH$}", row.keys), key_style),
                Span::styled(format!("{:ACTION_WIDTH$}", row.action), base),
                Span::styled(row.when, dim),
            ]))
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(menu.selected()));
    frame.render_stateful_widget(List::new(items), rows[1], &mut state);

    frame.render_widget(popup::separator(inner.width, theme), rows[2]);
    let hints = Line::from(vec![popup::key_pill("↑↓ PgUp PgDn", "scroll", theme.accent, theme), Span::raw("  "), popup::key_pill("esc", "close", theme.accent, theme)]);
    frame.render_widget(hints, rows[3]);
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_shows_keys_actions_and_when() {
        let menu = HotkeysMenu::open();
        let theme = Theme::dark();
        for style in [PopupStyle::Rounded, PopupStyle::Classic] {
            let text = crate::test_support::render_text(90, 60, |frame| draw_hotkeys(frame, frame.area(), &menu, &theme, style));
            assert!(text.contains("Hotkeys"), "{style:?}");
            assert!(text.contains("Key") && text.contains("Action") && text.contains("When"), "{style:?}");
            assert!(text.contains("Ctrl+O") && text.contains("Hide / show the panels"), "{style:?}");
        }
    }

    /// More rows than fit: the list scrolls to the highlight.
    #[test]
    fn a_small_screen_scrolls_to_the_highlight() {
        let mut menu = HotkeysMenu::open();
        let last = menu.rows().last().unwrap().action;
        menu.select_last_for_tests();
        let text = crate::test_support::render_text(90, 16, |frame| draw_hotkeys(frame, frame.area(), &menu, &Theme::dark(), PopupStyle::Rounded));
        assert!(text.contains(last), "{text}");
    }
}
