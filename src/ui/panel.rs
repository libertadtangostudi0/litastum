use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
    Frame,
};

use crate::explorer::{Entry, HighlightRole, Panel};
use crate::theming::Theme;

/// Renders one panel (border, path title, footer) and its column-major
/// file grid. Returns the `(columns, visible_rows)` actually used, so
/// the caller can feed both back into `Panel::set_columns`/
/// `set_visible_rows` — `visible_rows` is just `inner.height`, the same
/// number of text rows `draw_entry_grid` itself renders into for every
/// column (they're vertical slices of one shared height).
pub(super) fn draw_panel(frame: &mut Frame, area: Rect, panel: &Panel, is_active: bool, theme: &Theme) -> (usize, usize) {
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

    // Panels narrower than `panel_min_column_width` (per column) fall
    // back to a single column.
    let min_column_width = crate::theming::config::limits().panel_min_column_width;
    let columns = (inner.width / min_column_width).max(1) as usize;
    draw_entry_grid(frame, inner, panel, columns, is_active, theme);
    (columns, inner.height as usize)
}


/// Splits `area` into `columns` vertical slices and fills each with a
/// `panel.column_height()`-tall slice of the *currently visible page*
/// (`panel.scroll_offset()..` in the flat `entries` array) -- column 0
/// gets the page's own first `column_height` entries, column 1 the
/// next `column_height`, and so on, exactly the same math `Panel`'s own
/// navigation (`move_left`/`move_right`) uses, so the cursor and the
/// rendered grid always agree on which entry sits in which cell. See
/// `Panel::column_height`'s own doc comment for why this has to be
/// `min(area.height, the list's own even-split row count)`, not
/// `area.height` outright -- and `Panel`'s own struct doc for why
/// scrolling needs a real, tracked offset at all rather than handing
/// every row straight to a plain `List` (it doesn't scroll to follow
/// the cursor on its own, the original reported bug).
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


/// `is_marked` (Far Manager-style multi-select, `panel/marks.rs`)
/// overrides the entry's own type-based color entirely, matching real
/// Far Manager's own convention: a marked file/directory always renders
/// in the mark color, regardless of whether it's an archive, an
/// executable, or anything else `highlight_role` would otherwise pick.
/// Uses `theme.warning` -- the same "attention/marked" color the
/// original UI-theme plan already named for this
/// (`.claude/rules/litastum-ui-theme.md`) but never actually wired up
/// until marking itself existed.
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
