use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use crate::app::{App, Mode};
use crate::editor::Editor;
use crate::panel::{Entry, HighlightRole, Panel};
use crate::theme::Theme;
use crate::{command_line, confirm, find_file, menu, shell, theme_menu};

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
pub fn draw(frame: &mut Frame, app: &mut App) -> [usize; 2] {
    let theme = app.theme; // Theme is Copy -- see theme.rs for why
    let area = frame.area();
    match &mut app.mode {
        Mode::Editing(editor) => {
            draw_editor(frame, area, editor, &theme);
            return [1, 1];
        }
        Mode::ConfirmDiscard(editor) => {
            draw_editor(frame, area, editor, &theme);
            draw_confirm_discard_popup(frame, area, &theme);
            return [1, 1];
        }
        Mode::Browsing
        | Mode::MainMenu(_)
        | Mode::ThemeMenu(_)
        | Mode::ShellMenu(_)
        | Mode::ConfirmDelete(_)
        | Mode::ConfirmTransfer(_)
        | Mode::FindFile(_)
        | Mode::CommandHistory(_) => {}
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

    let left_columns = draw_panel(frame, panels[0], &app.panels[0], app.active == 0, &theme);
    let right_columns = draw_panel(frame, panels[1], &app.panels[1], app.active == 1, &theme);
    let shell_name = app.shell_profiles[app.active_shell].name.as_str();
    draw_command_line(frame, root[1], &app.command_line, shell_name, &theme);
    draw_function_keys(frame, root[2], &theme);

    // The real terminal cursor sits right after the typed text, same
    // mechanism already used for the editor's cursor (see
    // editor.rs::cursor_screen_position) -- only while nothing else is
    // drawn over the command line (a popup below takes visual priority,
    // and moving the cursor under it would be misleading).
    if matches!(app.mode, Mode::Browsing) {
        frame.set_cursor_position(Position {
            x: root[1].x + 2 + app.command_line.chars().count() as u16,
            y: root[1].y,
        });
    }

    // The F9/Ctrl+P popups show over the browser, like a Far Manager
    // menu, not in place of it -- unlike Editing/ConfirmDiscard above,
    // which replace the whole screen.
    match &app.mode {
        Mode::MainMenu(menu) => menu::draw_main_menu(frame, area, menu, &theme),
        Mode::ThemeMenu(menu) => theme_menu::draw_theme_menu(frame, area, menu, &theme),
        Mode::ShellMenu(menu) => shell::draw_shell_menu(frame, area, menu, &app.shell_profiles, &theme),
        Mode::ConfirmDelete(pending) => confirm::draw_confirm_delete_popup(frame, area, pending, &theme),
        Mode::ConfirmTransfer(pending) => {
            let cursor = confirm::draw_confirm_transfer_popup(frame, area, pending, &theme);
            frame.set_cursor_position(cursor);
        }
        Mode::FindFile(state) => {
            if let Some(cursor) = find_file::draw_find_file(frame, area, state, &theme) {
                frame.set_cursor_position(cursor);
            }
        }
        Mode::CommandHistory(menu) => {
            command_line::draw_command_history(frame, area, menu, &app.command_history, &theme);
        }
        _ => {}
    }

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

    let base_color = match entry.highlight_role() {
        HighlightRole::Parent => theme.text_dim,
        HighlightRole::Directory | HighlightRole::Other => theme.text,
        HighlightRole::VcsDirectory => theme.accent,
        HighlightRole::Archive => theme.warning,
        HighlightRole::Executable => theme.success,
    };
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


/// Renders the built-in editor full-screen (border/title drawn by
/// `Editor::view` itself), with a one-line hint bar below for its
/// special bindings (everything else goes straight to `edtui`).
fn draw_editor(frame: &mut Frame, area: Rect, editor: &mut Editor, theme: &Theme) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);

    let dirty_marker = if editor.is_dirty() { " [modified]" } else { "" };

    frame.render_widget(editor.view(theme), rows[0]);
    if let Some(pos) = editor.cursor_screen_position() {
        frame.set_cursor_position(pos);
    }

    let hint = Line::from(vec![
        Span::styled("Ctrl+S ", Style::default().fg(theme.accent)),
        Span::styled("Save   ", Style::default().fg(theme.text_dim)),
        Span::styled("Ctrl+C/X/V ", Style::default().fg(theme.accent)),
        Span::styled("Copy/Cut/Paste   ", Style::default().fg(theme.text_dim)),
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
/// `pub(crate)` since every popup drawer uses it, and most of those now
/// live in their own mode-owning module (`menu.rs`, `theme_menu.rs`,
/// `shell.rs`, `confirm.rs`) rather than here.
pub(crate) fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}


/// The always-live command line (Far Manager-style — see
/// `command_line.rs`). Shows the active shell profile's name at the
/// right edge, since which one a typed command actually runs against
/// is otherwise invisible (`Ctrl+P` to change it).
fn draw_command_line(frame: &mut Frame, area: Rect, command_line: &str, shell_name: &str, theme: &Theme) {
    let left = Line::from(vec![
        Span::styled("> ", Style::default().fg(theme.accent)),
        Span::styled(command_line.to_string(), Style::default().fg(theme.text)),
    ]);
    frame.render_widget(left, area);

    let label = format!("Ctrl+P {shell_name} ");
    let label_width = (label.chars().count() as u16).min(area.width);
    let label_area = Rect {
        x: area.x + area.width - label_width,
        y: area.y,
        width: label_width,
        height: 1,
    };
    let right = Line::from(Span::styled(label, Style::default().fg(theme.text_dim)));
    frame.render_widget(right, label_area);
}


fn draw_function_keys(frame: &mut Frame, area: Rect, theme: &Theme) {
    const LABELS: [(&str, &str); 10] = [
        ("F1", "Help"), ("F2", "Bookmarks"), ("F3", "View"), ("F4", "Edit"),
        ("F5", "Copy"), ("F6", "RenMov"), ("F7", "Folder"), ("F8", "Delete"),
        ("F9", "Menu"), ("F10", "Quit"),
    ];

    let spans: Vec<Span> = LABELS
        .iter()
        .flat_map(|(key, label)| {
            [
                Span::styled(format!("{key} "), Style::default().fg(theme.accent)),
                Span::styled(format!("{label}  "), Style::default().fg(theme.text_dim)),
            ]
        })
        .collect();

    frame.render_widget(Line::from(spans), area);
}
