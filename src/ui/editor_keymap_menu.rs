use ratatui::{layout::Rect, Frame};

use crate::editor::{EditorKeymapMenu, EditorKeymapMode};
use crate::theming::{PopupStyle, Theme};
use crate::ui::popup;

/// The editor's F9 -> Keybindings picker, drawn over the editor.
/// `current` is the editor's active mode, marked independently of the
/// highlighted row.
pub fn draw_editor_keymap_menu(frame: &mut Frame, area: Rect, menu: &EditorKeymapMenu, theme: &Theme, style: PopupStyle, current: EditorKeymapMode) {
    let labels = popup::choice_labels(menu, |mode| mode.label().to_string(), Some(current));
    popup::draw_list_popup(frame, area, theme, style, " Keybindings ", 30, &labels, menu.selected_index(), "apply", "cancel");
}


#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;
    use crate::choice_menu::ChoiceMenu;

    fn rendered(menu: &EditorKeymapMenu, style: PopupStyle, current: EditorKeymapMode) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        terminal
            .draw(|frame| {
                draw_editor_keymap_menu(frame, frame.area(), menu, &theme, style, current);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn lists_both_modes_and_marks_the_current_one() {
        let menu = ChoiceMenu::new(EditorKeymapMode::all(), Some(EditorKeymapMode::Standard));
        let text = rendered(&menu, PopupStyle::Rounded, EditorKeymapMode::Standard);
        assert!(text.contains("Standard (current)"));
        assert!(text.contains("Vim"));
    }

    #[test]
    fn renders_fine_in_classic_style_too() {
        let menu = ChoiceMenu::new(EditorKeymapMode::all(), Some(EditorKeymapMode::Vim));
        let text = rendered(&menu, PopupStyle::Classic, EditorKeymapMode::Vim);
        assert!(text.contains("Vim (current)"));
        assert!(text.contains("Standard"));
    }

    /// The "(current)" marker follows the editor's own *actually
    /// active* mode, not wherever the cursor happens to be sitting in
    /// the picker -- same split `popup_style_menu.rs` already relies on.
    #[test]
    fn current_marker_follows_the_active_mode_not_the_cursor() {
        let mut menu = ChoiceMenu::new(EditorKeymapMode::all(), Some(EditorKeymapMode::Standard));
        menu.move_down(); // cursor now on Vim, but Standard is still active
        let text = rendered(&menu, PopupStyle::Rounded, EditorKeymapMode::Standard);
        assert!(text.contains("Standard (current)"));
        assert!(!text.contains("Vim (current)"));
    }
}
