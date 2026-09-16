use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem},
    Frame,
};

use crate::editor::{EditorKeymapMenu, EditorKeymapMode};
use crate::theming::{PopupStyle, Theme};
use crate::ui::popup;

/// Renders the built-in editor's own F9 -> keybindings picker -- a
/// plain list of `EditorKeymapMode::all()`, same shape as
/// `ui/popup_style_menu.rs`'s own UI-style picker (which this was
/// directly modeled on). Drawn over the editor itself, which stays
/// visible underneath (`ui::draw`'s own `Mode::EditorKeymapMenu` arm).
/// `current` (the editor's own *actually active* mode -- `Editor::
/// keymap_mode()`) marks the "(current)" row independently of `menu`'s
/// own cursor position, same split popup_style_menu's own `style`
/// parameter already keeps.
pub fn draw_editor_keymap_menu(frame: &mut Frame, area: Rect, menu: &EditorKeymapMenu, theme: &Theme, style: PopupStyle, current: EditorKeymapMode) {
    let modes = EditorKeymapMode::all();
    let extra = popup::chrome_extra_rows(style);
    let height = (modes.len() as u16 + 4 + extra).clamp(6 + extra, area.height);
    let inner = popup::draw_frame(frame, area, theme, style, Line::from(Span::raw(" Keybindings ")), 30, height);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let items: Vec<ListItem> = modes
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            let label = if *candidate == current {
                format!("{} (current)", candidate.label())
            } else {
                candidate.label().to_string()
            };
            let item_style = if index == menu.selected {
                popup::selected_row_style(theme)
            } else {
                Style::default().fg(theme.text)
            };
            ListItem::new(Line::from(Span::styled(label, item_style)))
        })
        .collect();
    frame.render_widget(List::new(items), rows[0]);

    let hint = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" apply  ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(theme.text_dim)),
    ]);
    frame.render_widget(hint, rows[1]);
}


#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;

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
        let menu = EditorKeymapMenu::open(EditorKeymapMode::Standard);
        let text = rendered(&menu, PopupStyle::Rounded, EditorKeymapMode::Standard);
        assert!(text.contains("Standard (current)"));
        assert!(text.contains("Vim"));
    }

    #[test]
    fn renders_fine_in_classic_style_too() {
        let menu = EditorKeymapMenu::open(EditorKeymapMode::Vim);
        let text = rendered(&menu, PopupStyle::Classic, EditorKeymapMode::Vim);
        assert!(text.contains("Vim (current)"));
        assert!(text.contains("Standard"));
    }

    /// The "(current)" marker follows the editor's own *actually
    /// active* mode, not wherever the cursor happens to be sitting in
    /// the picker -- same split `popup_style_menu.rs` already relies on.
    #[test]
    fn current_marker_follows_the_active_mode_not_the_cursor() {
        let mut menu = EditorKeymapMenu::open(EditorKeymapMode::Standard);
        menu.move_down(); // cursor now on Vim, but Standard is still active
        let text = rendered(&menu, PopupStyle::Rounded, EditorKeymapMode::Standard);
        assert!(text.contains("Standard (current)"));
        assert!(!text.contains("Vim (current)"));
    }
}
