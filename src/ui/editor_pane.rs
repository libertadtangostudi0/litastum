use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::editor::Editor;
use crate::theming::Theme;

use super::centered_rect;

/// Renders the built-in editor full-screen (border/title drawn by
/// `Editor::view` itself), with a one-line hint bar below for its
/// special bindings (everything else goes straight to `edtui`).
///
/// Returns the real terminal cursor's own desired position rather than
/// calling `frame.set_cursor_position` itself -- see `ui/mod.rs::draw`'s
/// own doc comment on why every cursor-placing draw function in this
/// app returns it instead now, up to `main.rs::run`, which applies it
/// once, after the whole frame has actually reached the terminal.
pub(super) fn draw_editor(frame: &mut Frame, area: Rect, editor: &mut Editor, theme: &Theme) -> Option<Position> {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);

    let dirty_marker = if editor.is_dirty() { " [modified]" } else { "" };

    frame.render_widget(editor.view(theme, rows[0]), rows[0]);
    let cursor = editor.cursor_screen_position();

    let hint = Line::from(vec![
        Span::styled("Ctrl+S ", Style::default().fg(theme.accent)),
        Span::styled("Save   ", Style::default().fg(theme.text_dim)),
        Span::styled("Ctrl+F ", Style::default().fg(theme.accent)),
        Span::styled("Find   ", Style::default().fg(theme.text_dim)),
        Span::styled("Ctrl+C/X/V ", Style::default().fg(theme.accent)),
        Span::styled("Copy/Cut/Paste   ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc ", Style::default().fg(theme.accent)),
        Span::styled("Close", Style::default().fg(theme.text_dim)),
        Span::styled(dirty_marker, Style::default().fg(theme.danger)),
    ]);
    frame.render_widget(hint, rows[1]);
    cursor
}


/// Renders the "discard unsaved changes?" prompt centered over
/// whatever's already drawn (the editor, still visible underneath).
pub(super) fn draw_confirm_discard_popup(frame: &mut Frame, area: Rect, theme: &Theme) {
    let popup = centered_rect(44, 4, area);

    frame.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.danger))
        .title(" Unsaved changes ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let lines = vec![
        Line::from(Span::styled("Discard unsaved changes?", Style::default().fg(theme.text))),
        Line::from(vec![
            Span::styled("Y", Style::default().fg(theme.danger).add_modifier(Modifier::BOLD)),
            Span::styled(" discard    ", Style::default().fg(theme.text_dim)),
            Span::styled("N", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
            Span::styled(" / Esc cancel", Style::default().fg(theme.text_dim)),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}
