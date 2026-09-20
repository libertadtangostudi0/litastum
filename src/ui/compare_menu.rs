use ratatui::{layout::Rect, Frame};

use crate::compare::CompareMenu;
use crate::theming::{PopupStyle, Theme};
use crate::ui::popup;

/// Renders Compare's own F9 menu -- same shape as `ui/editor_menu.rs`'s
/// own F9 (which this was directly modeled on).
pub fn draw_compare_menu(frame: &mut Frame, area: Rect, menu: &CompareMenu, theme: &Theme, style: PopupStyle) {
    let labels: Vec<String> = menu.items().iter().map(|label| (*label).to_string()).collect();
    popup::draw_list_popup(frame, area, theme, style, " Menu ", 30, &labels, menu.selected, "open", "close");
}


#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;

    fn rendered(menu: &CompareMenu, style: PopupStyle) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        terminal
            .draw(|frame| {
                draw_compare_menu(frame, frame.area(), menu, &theme, style);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn shows_the_line_endings_item() {
        let menu = CompareMenu::open();
        let text = rendered(&menu, PopupStyle::Rounded);
        assert!(text.contains("Line endings"));
    }

    #[test]
    fn renders_fine_in_classic_style_too() {
        let menu = CompareMenu::open();
        let text = rendered(&menu, PopupStyle::Classic);
        assert!(text.contains("Line endings"));
    }
}
