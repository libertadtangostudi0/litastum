use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem},
    Frame,
};

use crate::editor::EditorMenu;
use crate::theming::{PopupStyle, Theme};
use crate::ui::popup;

/// Renders the built-in editor's own F9 menu -- same shape as
/// `ui/menu.rs::draw_main_menu` (the browsing screen's own F9), just
/// over a single, currently one-item list (`EditorMenu::items`) rather
/// than `MenuLevel`'s multi-level one.
pub fn draw_editor_menu(frame: &mut Frame, area: Rect, menu: &EditorMenu, theme: &Theme, style: PopupStyle) {
    let items = menu.items();
    let extra = popup::chrome_extra_rows(style);
    let height = (items.len() as u16 + 4 + extra).clamp(6 + extra, area.height);
    let inner = popup::draw_frame(frame, area, theme, style, Line::from(Span::raw(" Menu ")), 30, height);

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
        Span::styled(" close", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[1]);
}


#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;

    fn rendered(menu: &EditorMenu, style: PopupStyle) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        terminal
            .draw(|frame| {
                draw_editor_menu(frame, frame.area(), menu, &theme, style);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn shows_the_keybindings_item() {
        let menu = EditorMenu::open();
        let text = rendered(&menu, PopupStyle::Rounded);
        assert!(text.contains("Keybindings"));
    }

    #[test]
    fn renders_fine_in_classic_style_too() {
        let menu = EditorMenu::open();
        let text = rendered(&menu, PopupStyle::Classic);
        assert!(text.contains("Keybindings"));
    }
}
