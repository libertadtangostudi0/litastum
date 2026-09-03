use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
    Frame,
};

use crate::app::App;
use crate::panel::Panel;


/// Draws the whole application: two file panels side by side, a
/// command-line row, and the F-key hint bar.
pub fn draw(frame: &mut Frame, app: &App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let panels = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(root[0]);

    draw_panel(frame, panels[0], &app.panels[0], app.active == 0);
    draw_panel(frame, panels[1], &app.panels[1], app.active == 1);
    draw_command_line(frame, root[1]);
    draw_function_keys(frame, root[2]);
}


fn draw_panel(frame: &mut Frame, area: Rect, panel: &Panel, is_active: bool) {
    let border_style = if is_active {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let items: Vec<ListItem> = panel
        .entries
        .iter()
        .enumerate()
        .map(|(i, entry)| build_list_item(entry, i == panel.selected))
        .collect();

    let title = panel.path.to_string_lossy().into_owned();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title);

    frame.render_widget(List::new(items).block(block), area);
}


fn build_list_item(entry: &crate::panel::Entry, is_selected: bool) -> ListItem<'static> {
    let label = if entry.is_dir {
        format!("{}/", entry.name)
    } else {
        entry.name.clone()
    };

    let style = if is_selected {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    ListItem::new(Line::from(Span::styled(label, style)))
}


fn draw_command_line(frame: &mut Frame, area: Rect) {
    let line = Line::from(Span::styled("> ", Style::default().fg(Color::Cyan)));
    frame.render_widget(line, area);
}


fn draw_function_keys(frame: &mut Frame, area: Rect) {
    let labels = [
        "F1 Help", "F2 Bookmarks", "F3 View", "F4 Edit", "F5 Copy",
        "F6 Move", "F7 Folder", "F8 Delete", "F9 Menu", "F10 Quit",
    ];
    let line = Line::from(labels.join("  "));
    frame.render_widget(line, area);
}
