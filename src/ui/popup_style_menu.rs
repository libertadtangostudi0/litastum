use ratatui::{layout::Rect, Frame};

use crate::theming::{PopupStyle, PopupStyleMenu, Theme};
use crate::ui::popup;

/// Renders the F9 -> Options -> UI popup-style picker -- a plain list
/// of `PopupStyle::all()`, same shape as `ui/shell.rs`'s profile
/// picker. Drawn using `style` itself (the style still active *while*
/// picking, before `Enter` commits a new one) -- same as every other
/// popup in this app always rendering with whatever's currently active.
pub fn draw_popup_style_menu(frame: &mut Frame, area: Rect, menu: &PopupStyleMenu, theme: &Theme, style: PopupStyle) {
    let labels: Vec<String> = PopupStyle::all()
        .iter()
        .map(|candidate| if *candidate == style { format!("{} (current)", candidate.label()) } else { candidate.label().to_string() })
        .collect();
    popup::draw_list_popup(frame, area, theme, style, " UI ", 30, &labels, menu.selected, "apply", "cancel");
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
