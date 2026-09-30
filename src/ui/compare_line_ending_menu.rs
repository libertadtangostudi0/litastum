use ratatui::{layout::Rect, Frame};

use crate::compare::{CompareLineEndingMenu, LineEndingDisplay};
use crate::theming::{PopupStyle, Theme};
use crate::ui::popup;

/// Compare's F9 -> Line endings picker. `current`
/// (`App::settings.compare_line_ending_display`) is marked independently of the
/// highlighted row.
pub fn draw_compare_line_ending_menu(frame: &mut Frame, area: Rect, menu: &CompareLineEndingMenu, theme: &Theme, style: PopupStyle, current: LineEndingDisplay) {
    let labels = popup::choice_labels(menu, |display| display.label().to_string(), Some(current));
    popup::draw_list_popup(frame, area, theme, style, " Line endings ", 30, &labels, menu.selected_index(), "apply", "cancel");
}


#[cfg(test)]
mod tests {
    use crate::test_support::buffer_text;
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;
    use crate::choice_menu::ChoiceMenu;

    fn rendered(menu: &CompareLineEndingMenu, style: PopupStyle, current: LineEndingDisplay) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        terminal
            .draw(|frame| {
                draw_compare_line_ending_menu(frame, frame.area(), menu, &theme, style, current);
            })
            .unwrap();
        buffer_text(terminal.backend().buffer())
    }

    #[test]
    fn lists_both_choices_and_marks_the_current_one() {
        let menu = ChoiceMenu::new(LineEndingDisplay::all(), Some(LineEndingDisplay::Hidden));
        let text = rendered(&menu, PopupStyle::Rounded, LineEndingDisplay::Hidden);
        assert!(text.contains("Hidden (current)"));
        assert!(text.contains("Shown"));
    }

    #[test]
    fn renders_fine_in_classic_style_too() {
        let menu = ChoiceMenu::new(LineEndingDisplay::all(), Some(LineEndingDisplay::Shown));
        let text = rendered(&menu, PopupStyle::Classic, LineEndingDisplay::Shown);
        assert!(text.contains("Shown (current)"));
        assert!(text.contains("Hidden"));
    }
}
