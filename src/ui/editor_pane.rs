use ratatui::{
    layout::{Constraint, Layout, Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    Frame,
};

use crate::editor::Editor;
use crate::theming::{PopupStyle, Theme};

use super::popup;

/// The editor over the whole area (`Editor::view` draws the border and
/// the title, `[modified]` included). No hint row: requested, the screen
/// is the editor's. Returns the cursor position rather than setting it
/// (see `ui::draw`).
pub(super) fn draw_editor(frame: &mut Frame, area: Rect, editor: &mut Editor, theme: &Theme) -> Option<Position> {
    frame.render_widget(editor.view(theme, area), area);
    editor.cursor_screen_position()
}


/// The "discard unsaved changes?" prompt over the editor, Compare or the
/// resolver, still visible underneath. Built on `popup::draw_frame` like
/// the delete prompt, so it follows F9 -> Options -> UI (reported: it
/// stayed square under `Rounded`, being hand-drawn).
pub(super) fn draw_confirm_discard_popup(frame: &mut Frame, area: Rect, theme: &Theme, style: PopupStyle) {
    const WIDTH: u16 = 44;
    // The message, a separator, the key pills; plus the border and
    // whatever `style` adds.
    let height = 3 + 2 + popup::chrome_extra_rows(style);
    let title = match style {
        PopupStyle::Classic => Line::from(Span::raw(" Unsaved changes ")),
        PopupStyle::Rounded => Line::from(vec![
            Span::styled("● ", Style::default().fg(theme.danger)),
            Span::styled("Unsaved changes", Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
        ]),
    };
    let inner = popup::draw_frame(frame, area, theme, style, title, WIDTH, height);

    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)]).split(inner);
    frame.render_widget(Line::from(Span::styled("Discard unsaved changes?", Style::default().fg(theme.text))), rows[0]);
    frame.render_widget(popup::separator(inner.width, theme), rows[1]);
    let hints = Line::from(vec![popup::key_pill("y", "discard", theme.danger, theme), Span::raw("  "), popup::key_pill("esc", "cancel", theme.accent, theme)]);
    frame.render_widget(hints, rows[2]);
}


#[cfg(test)]
mod confirm_discard_tests {
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;
    use crate::test_support::buffer_text;

    fn rendered(style: PopupStyle) -> String {
        let mut terminal = Terminal::new(TestBackend::new(60, 16)).unwrap();
        terminal.draw(|frame| draw_confirm_discard_popup(frame, frame.area(), &Theme::dark(), style)).unwrap();
        buffer_text(terminal.backend().buffer())
    }

    #[test]
    fn the_discard_prompt_follows_the_popup_style() {
        let rounded = rendered(PopupStyle::Rounded);
        assert!(rounded.contains('╭'), "{rounded}");
        assert!(rounded.contains("Discard unsaved changes?") && rounded.contains("discard") && rounded.contains("cancel"));

        let classic = rendered(PopupStyle::Classic);
        assert!(!classic.contains('╭') && classic.contains("Unsaved changes"), "{classic}");
        assert!(classic.contains("Discard unsaved changes?"));
    }
}
