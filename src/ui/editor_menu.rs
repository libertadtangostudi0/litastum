use ratatui::{layout::Rect, Frame};

use crate::editor::EditorMenu;
use crate::theming::{PopupStyle, Theme};
use crate::ui::popup;

/// Renders the built-in editor's own F9 menu -- same shape as
/// `ui/menu.rs::draw_main_menu` (the browsing screen's own F9), just
/// over a single, currently one-item list (`EditorMenu::items`) rather
/// than `MenuLevel`'s multi-level one.
pub fn draw_editor_menu(frame: &mut Frame, area: Rect, menu: &EditorMenu, theme: &Theme, style: PopupStyle) {
    let labels: Vec<String> = menu.items().iter().map(|label| (*label).to_string()).collect();
    popup::draw_list_popup(frame, area, theme, style, " Menu ", 30, &labels, menu.selected, "open", "close");
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
