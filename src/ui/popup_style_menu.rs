use ratatui::{layout::Rect, Frame};

use crate::theming::{PopupStyle, PopupStyleMenu, Theme};
use crate::ui::popup;

/// The F9 -> Options -> UI picker, drawn in the style still active while
/// picking; the active style is marked "(current)".
pub fn draw_popup_style_menu(frame: &mut Frame, area: Rect, menu: &PopupStyleMenu, theme: &Theme, style: PopupStyle) {
    let labels = popup::choice_labels(menu, |candidate| candidate.label().to_string(), Some(style));
    popup::draw_list_popup(frame, area, theme, style, " UI ", 30, &labels, menu.selected_index(), "apply", "cancel");
}


#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;
    use crate::choice_menu::ChoiceMenu;

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
        let menu = ChoiceMenu::new(PopupStyle::all(), Some(PopupStyle::Rounded));
        let text = rendered(&menu, PopupStyle::Rounded);
        assert!(text.contains("Classic"));
        assert!(text.contains("Rounded (current)"));
    }

    #[test]
    fn renders_fine_in_classic_style_too() {
        let menu = ChoiceMenu::new(PopupStyle::all(), Some(PopupStyle::Classic));
        let text = rendered(&menu, PopupStyle::Classic);
        assert!(text.contains("Classic (current)"));
        assert!(text.contains("Rounded"));
    }
}
