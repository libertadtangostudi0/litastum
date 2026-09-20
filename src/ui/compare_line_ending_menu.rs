use ratatui::{layout::Rect, Frame};

use crate::compare::{CompareLineEndingMenu, LineEndingDisplay};
use crate::theming::{PopupStyle, Theme};
use crate::ui::popup;

/// Renders Compare's own F9 -> Line endings picker -- same shape as
/// `ui/editor_keymap_menu.rs`'s own `Standard`/`Vim` picker (which this
/// was directly modeled on). `current` (`App::compare_line_ending_display`)
/// marks the "(current)" row independently of `menu`'s own cursor
/// position, same split that picker's own `current` parameter already
/// keeps.
pub fn draw_compare_line_ending_menu(frame: &mut Frame, area: Rect, menu: &CompareLineEndingMenu, theme: &Theme, style: PopupStyle, current: LineEndingDisplay) {
    let labels: Vec<String> = LineEndingDisplay::all()
        .iter()
        .map(|candidate| if *candidate == current { format!("{} (current)", candidate.label()) } else { candidate.label().to_string() })
        .collect();
    popup::draw_list_popup(frame, area, theme, style, " Line endings ", 30, &labels, menu.selected, "apply", "cancel");
}


#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;

    fn rendered(menu: &CompareLineEndingMenu, style: PopupStyle, current: LineEndingDisplay) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        terminal
            .draw(|frame| {
                draw_compare_line_ending_menu(frame, frame.area(), menu, &theme, style, current);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn lists_both_choices_and_marks_the_current_one() {
        let menu = CompareLineEndingMenu::open(LineEndingDisplay::Hidden);
        let text = rendered(&menu, PopupStyle::Rounded, LineEndingDisplay::Hidden);
        assert!(text.contains("Hidden (current)"));
        assert!(text.contains("Shown"));
    }

    #[test]
    fn renders_fine_in_classic_style_too() {
        let menu = CompareLineEndingMenu::open(LineEndingDisplay::Shown);
        let text = rendered(&menu, PopupStyle::Classic, LineEndingDisplay::Shown);
        assert!(text.contains("Shown (current)"));
        assert!(text.contains("Hidden"));
    }
}
