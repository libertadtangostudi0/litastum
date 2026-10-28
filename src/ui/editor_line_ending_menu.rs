use ratatui::{layout::Rect, Frame};

use crate::editor::{EditorLineEndingMenu, LineEnding};
use crate::theming::{PopupStyle, Theme};
use crate::ui::popup;

/// The editor's F9 -> Line endings picker, drawn over the editor.
/// `current` is the line break a save would write, marked apart from the
/// highlighted row.
pub fn draw_editor_line_ending_menu(frame: &mut Frame, area: Rect, menu: &EditorLineEndingMenu, theme: &Theme, style: PopupStyle, current: LineEnding) {
    let labels = popup::choice_labels(menu, |ending| ending.description().to_string(), Some(current));
    popup::draw_list_popup(frame, area, theme, style, " Line endings ", 34, &labels, menu.selected_index(), "apply", "cancel");
}


#[cfg(test)]
mod tests {
    use crate::test_support::render_text;

    use super::*;
    use crate::choice_menu::ChoiceMenu;

    #[test]
    fn lists_both_and_marks_the_current_one() {
        let theme = Theme::dark();
        let mut menu = ChoiceMenu::new(LineEnding::ALL, Some(LineEnding::CrLf));
        menu.move_up();
        let text = render_text(80, 24, |frame| draw_editor_line_ending_menu(frame, frame.area(), &menu, &theme, PopupStyle::Rounded, LineEnding::CrLf));
        assert!(text.contains("CRLF (Windows) (current)"), "{text}");
        assert!(text.contains("LF (Unix, macOS)"));
        assert!(!text.contains("macOS) (current)"), "the mark follows the file, not the highlight");
    }
}
