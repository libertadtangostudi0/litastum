use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
    Frame,
};

use crate::explorer::{Entry, HighlightRole, Panel};
use crate::theming::Theme;

/// Renders one panel (border, path title -- its end kept when it doesn't
/// fit, footer) and its column-major
/// file grid. Returns the `(columns, visible_rows)` actually used, so
/// the caller can feed both back into `Panel::set_columns`/
/// `set_visible_rows` — `visible_rows` is just `inner.height`, the same
/// number of text rows `draw_entry_grid` itself renders into for every
/// column (they're vertical slices of one shared height).
pub(super) fn draw_panel(frame: &mut Frame, area: Rect, panel: &Panel, is_active: bool, theme: &Theme) -> (usize, usize) {
    draw_panel_in_columns(frame, area, panel, is_active, theme, None)
}


/// `draw_panel` with `columns` columns, else as many as fit.
pub(super) fn draw_panel_in_columns(frame: &mut Frame, area: Rect, panel: &Panel, is_active: bool, theme: &Theme, columns: Option<usize>) -> (usize, usize) {
    let border_style = if is_active {
        Style::default().fg(theme.accent)
    } else {
        Style::default().fg(theme.border)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(crate::editor::fitted_title(&panel.path, area.width));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Panels narrower than `panel_min_column_width` (per column) fall
    // back to a single column.
    let min_column_width = crate::theming::config::limits().panel_min_column_width;
    let columns = columns.unwrap_or((inner.width / min_column_width).max(1) as usize);
    draw_entry_grid(frame, inner, panel, columns, is_active, theme);
    (columns, inner.height as usize)
}


/// Splits `area` into `columns` slices, each holding `column_height()`
/// entries of the visible page from `scroll_offset()` -- the same math as
/// `Panel`'s navigation, so the cursor and the grid agree. (A plain `List`
/// doesn't follow the cursor, hence the tracked offset.)
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

    let column_height = panel.column_height();
    if column_height == 0 {
        return;
    }
    let column_areas = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![Constraint::Ratio(1, columns as u32); columns])
        .split(area);

    for (col_index, &column_area) in column_areas.iter().enumerate() {
        let col_start = panel.scroll_offset() + col_index * column_height;
        if col_start >= panel.entries.len() {
            continue;
        }
        let col_end = (col_start + column_height).min(panel.entries.len());

        let items: Vec<ListItem> = panel.entries[col_start..col_end]
            .iter()
            .enumerate()
            .map(|(row_offset, entry)| {
                let global_index = col_start + row_offset;
                build_list_item(entry, global_index == panel.selected, panel.is_marked(global_index), is_active, theme)
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


/// How wide `panel` needs to be for one column of its names, borders
/// included: the widest name (a directory's with its `/`), at least
/// `panel_min_column_width`, at most `widest`.
pub(super) fn one_column_width(panel: &Panel, widest: u16) -> u16 {
    use unicode_width::UnicodeWidthStr;

    let names = panel.entries.iter().map(|entry| entry.name.width() + usize::from(entry.is_dir)).max().unwrap_or(0);
    let min_column_width = crate::theming::config::limits().panel_min_column_width;
    let wanted = u16::try_from(names + 2).unwrap_or(u16::MAX).max(min_column_width + 2);
    wanted.min(widest)
}


/// A marked entry is drawn in `theme.warning` whatever its type, as in
/// Far.
pub(super) fn build_list_item(entry: &Entry, is_selected: bool, is_marked: bool, panel_active: bool, theme: &Theme) -> ListItem<'static> {
    let label = if entry.is_dir {
        format!("{}/", entry.name)
    } else {
        entry.name.clone()
    };

    let base_color = if is_marked {
        theme.warning
    } else {
        match entry.highlight_role() {
            HighlightRole::Parent => theme.text_dim,
            HighlightRole::Directory | HighlightRole::Other => theme.text,
            HighlightRole::VcsDirectory => theme.accent,
            HighlightRole::Archive => theme.warning,
            HighlightRole::Executable => theme.success,
        }
    };
    let mut style = Style::default().fg(base_color);
    if is_marked {
        style = style.add_modifier(Modifier::BOLD);
    }
    if is_selected {
        style = if panel_active {
            // `theme.selection_text` overrides the entry's own type
            // color only when a scheme actually asks for it (see its
            // own doc comment) -- every other scheme keeps today's
            // behavior of leaving the file-type color alone here.
            let mut selected_style = style.bg(theme.current_row_bg).add_modifier(Modifier::BOLD);
            if let Some(selection_text) = theme.selection_text {
                selected_style = selected_style.fg(selection_text);
            }
            selected_style
        } else {
            style.add_modifier(Modifier::UNDERLINED)
        };
    }

    ListItem::new(Line::from(Span::styled(label, style)))
}
