use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use crate::app::{App, Mode};
use crate::editor::Editor;
use crate::panel::{Entry, Panel};
use crate::theme::Theme;

/// Panels narrower than this (per column) fall back to a single column.
const MIN_COLUMN_WIDTH: u16 = 24;


/// Draws the whole application: two file panels side by side, a
/// command-line row, and the F-key hint bar.
///
/// Returns the column count each panel was actually rendered with, so
/// the caller can feed it back into `Panel::set_columns` before the next
/// keyboard event is handled — column count depends on terminal size,
/// which only `ui::draw` computes, but `Panel` (not `ui`) owns the
/// cursor state that navigation needs it for.
pub fn draw(frame: &mut Frame, app: &App, theme: &Theme) -> [usize; 2] {
    match &app.mode {
        Mode::Editing(editor) => {
            draw_editor(frame, frame.area(), editor, theme);
            return [1, 1];
        }
        Mode::ConfirmDiscard(editor) => {
            draw_editor(frame, frame.area(), editor, theme);
            draw_confirm_discard_popup(frame, frame.area(), theme);
            return [1, 1];
        }
        Mode::Browsing => {}
    }

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

    let left_columns = draw_panel(frame, panels[0], &app.panels[0], app.active == 0, theme);
    let right_columns = draw_panel(frame, panels[1], &app.panels[1], app.active == 1, theme);
    draw_command_line(frame, root[1], theme);
    draw_function_keys(frame, root[2], theme);

    [left_columns, right_columns]
}


/// Renders one panel (border, path title, footer) and its column-major
/// file grid. Returns the column count used.
fn draw_panel(frame: &mut Frame, area: Rect, panel: &Panel, is_active: bool, theme: &Theme) -> usize {
    let border_style = if is_active {
        Style::default().fg(theme.accent)
    } else {
        Style::default().fg(theme.border)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(panel.path.to_string_lossy().into_owned());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let columns = (inner.width / MIN_COLUMN_WIDTH).max(1) as usize;
    draw_entry_grid(frame, inner, panel, columns, is_active, theme);
    columns
}


/// Splits `area` into `columns` vertical slices and fills each with the
/// entries belonging to that column (column-major: column 0 holds
/// entries `0..rows`, column 1 holds `rows..2*rows`, etc).
fn draw_entry_grid(
    frame: &mut Frame,
    area: Rect,
    panel: &Panel,
    columns: usize,
    is_active: bool,
    theme: &Theme,
) {
    if panel.entries.is_empty() {
        return;
    }

    let rows = panel.entries.len().div_ceil(columns);
    let column_areas = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![Constraint::Ratio(1, columns as u32); columns])
        .split(area);

    for (col_index, &column_area) in column_areas.iter().enumerate() {
        let start = col_index * rows;
        if start >= panel.entries.len() {
            continue;
        }
        let end = (start + rows).min(panel.entries.len());

        let items: Vec<ListItem> = panel.entries[start..end]
            .iter()
            .enumerate()
            .map(|(row_index, entry)| {
                let global_index = start + row_index;
                build_list_item(entry, global_index == panel.selected, is_active, theme)
            })
            .collect();

        let divider = if col_index + 1 < columns {
            Block::default()
                .borders(Borders::RIGHT)
                .border_style(Style::default().fg(theme.border))
        } else {
            Block::default()
        };

        frame.render_widget(List::new(items).block(divider), column_area);
    }
}


fn build_list_item(entry: &Entry, is_selected: bool, panel_active: bool, theme: &Theme) -> ListItem<'static> {
    let label = if entry.is_dir {
        format!("{}/", entry.name)
    } else {
        entry.name.clone()
    };

    let base_color = if entry.name == ".." { theme.text_dim } else { theme.text };
    let mut style = Style::default().fg(base_color);
    if is_selected {
        style = if panel_active {
            style.bg(theme.current_row_bg).add_modifier(Modifier::BOLD)
        } else {
            style.add_modifier(Modifier::UNDERLINED)
        };
    }

    ListItem::new(Line::from(Span::styled(label, style)))
}


/// Renders the built-in editor full-screen, with a one-line hint bar
/// for its two special bindings (everything else goes to the text area).
fn draw_editor(frame: &mut Frame, area: Rect, editor: &Editor, theme: &Theme) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);

    frame.render_widget(editor.widget(), rows[0]);

    let dirty_marker = if editor.is_dirty() { " [modified]" } else { "" };
    let hint = Line::from(vec![
        Span::styled("Ctrl+S ", Style::default().fg(theme.accent)),
        Span::styled("Save   ", Style::default().fg(theme.text_dim)),
        Span::styled("Esc ", Style::default().fg(theme.accent)),
        Span::styled("Close", Style::default().fg(theme.text_dim)),
        Span::styled(dirty_marker, Style::default().fg(theme.danger)),
    ]);
    frame.render_widget(hint, rows[1]);
}


/// Renders the "discard unsaved changes?" prompt centered over
/// whatever's already drawn (the editor, still visible underneath).
fn draw_confirm_discard_popup(frame: &mut Frame, area: Rect, theme: &Theme) {
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


/// A `width`x`height` rectangle centered within `area`, clamped to fit.
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}


fn draw_command_line(frame: &mut Frame, area: Rect, theme: &Theme) {
    let line = Line::from(Span::styled("> ", Style::default().fg(theme.accent)));
    frame.render_widget(line, area);
}


fn draw_function_keys(frame: &mut Frame, area: Rect, theme: &Theme) {
    const LABELS: [(&str, &str); 10] = [
        ("F1", "Help"), ("F2", "Bookmarks"), ("F3", "View"), ("F4", "Edit"),
        ("F5", "Copy"), ("F6", "Move"), ("F7", "Folder"), ("F8", "Delete"),
        ("F9", "Menu"), ("F10", "Quit"),
    ];

    let spans: Vec<Span> = LABELS
        .iter()
        .flat_map(|(key, label)| {
            let key_color = if *key == "F8" { theme.danger } else { theme.accent };
            [
                Span::styled(format!("{key} "), Style::default().fg(key_color)),
                Span::styled(format!("{label}  "), Style::default().fg(theme.text_dim)),
            ]
        })
        .collect();

    frame.render_widget(Line::from(spans), area);
}
