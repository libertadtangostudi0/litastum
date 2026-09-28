use ratatui::{layout::Rect, Frame};

use crate::compare::CompareMenu;
use crate::theming::{PopupStyle, Theme};
use crate::ui::popup;

/// Compare's own F9 menu, drawn over Compare.
pub fn draw_compare_menu(frame: &mut Frame, area: Rect, menu: &CompareMenu, theme: &Theme, style: PopupStyle) {
    let labels = popup::choice_labels(menu, |item| item.label().to_string(), None);
    popup::draw_list_popup(frame, area, theme, style, " Menu ", 30, &labels, menu.selected_index(), "open", "close");
}


#[cfg(test)]
mod tests {
    use crate::test_support::buffer_text;
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;
    use crate::choice_menu::ChoiceMenu;
    use crate::compare::CompareMenuItem;

    fn rendered(menu: &CompareMenu, style: PopupStyle) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        terminal
            .draw(|frame| {
                draw_compare_menu(frame, frame.area(), menu, &theme, style);
            })
            .unwrap();
        buffer_text(terminal.backend().buffer())
    }

    #[test]
    fn shows_the_line_endings_item() {
        let menu = ChoiceMenu::new(CompareMenuItem::ALL, None);
        let text = rendered(&menu, PopupStyle::Rounded);
        assert!(text.contains("Line endings"));
    }

    #[test]
    fn renders_fine_in_classic_style_too() {
        let menu = ChoiceMenu::new(CompareMenuItem::ALL, None);
        let text = rendered(&menu, PopupStyle::Classic);
        assert!(text.contains("Line endings"));
    }
}
