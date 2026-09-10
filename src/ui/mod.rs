use std::path::Path;

use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use crate::app::{App, Mode};
use crate::editor::Editor;
use crate::explorer::{Entry, HighlightRole, Panel};
use crate::theming::Theme;

mod command_line;
mod confirm;
mod drive_menu;
mod editor_find;
mod find_file;
mod menu;
mod popup;
mod shell;
mod theme_menu;

/// Panels narrower than this (per column) fall back to a single column.
const MIN_COLUMN_WIDTH: u16 = 24;


/// Draws the whole application: two file panels side by side, a
/// command-line row, and the F-key hint bar.
///
/// Returns the `(columns, visible_rows)` each panel was actually
/// rendered with, so the caller can feed it back into
/// `Panel::set_columns`/`set_visible_rows` before the next keyboard
/// event is handled — both depend on terminal size, which only
/// `ui::draw` computes, but `Panel` (not `ui`) owns the cursor/scroll
/// state that navigation needs them for.
pub fn draw(frame: &mut Frame, app: &mut App) -> [(usize, usize); 2] {
    let theme = app.theme; // Theme is Copy -- see theme.rs for why
    let area = frame.area();
    match &mut app.mode {
        Mode::Editing(editor) => {
            draw_editor(frame, area, editor, &theme);
            if editor.is_searching() {
                // Drawn on top, same "popup over a full-screen mode"
                // shape as ConfirmDiscard below -- and takes over the
                // real terminal cursor from draw_editor's own buffer-
                // cursor placement, same reasoning as the command line's
                // own cursor yielding to whichever popup is showing.
                let cursor = editor_find::draw_find_popup(frame, area, editor, &app.search_history, &theme);
                frame.set_cursor_position(cursor);
            }
            return [(1, 1), (1, 1)];
        }
        Mode::ConfirmDiscard(editor) => {
            draw_editor(frame, area, editor, &theme);
            draw_confirm_discard_popup(frame, area, &theme);
            return [(1, 1), (1, 1)];
        }
        Mode::Browsing
        | Mode::MainMenu(_)
        | Mode::ThemeMenu(_)
        | Mode::ShellMenu(_)
        | Mode::ConfirmDelete(_)
        | Mode::ConfirmTransfer(_)
        | Mode::FindFile(_)
        | Mode::CommandHistory(_)
        | Mode::ChangeDrive(_) => {}
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
    let cwd = app.panels[app.active].path.clone();
    let prefix_len = draw_command_line(frame, root[1], &cwd, &app.command_line, app.command_line_selection_anchor, app.command_line_cursor, &theme);
    draw_function_keys(frame, root[2], &theme, app.alt_held);

    // Auto-popping history suggestions, Far Manager-style: shown right
    // above the command line the instant there's a substring match,
    // no explicit key needed to open it (unlike the Alt+F8 popup,
    // which stays as an always-available manual search). Only in
    // Mode::Browsing -- once a popup/mode below has its own meaning
    // for the command line (or none at all), this shouldn't also be
    // showing over it.
    if matches!(app.mode, Mode::Browsing) && !app.command_line_suggestion_dismissed {
        let suggestions = crate::command_line::suggest_history(&app.command_history, &app.command_line);
        if !suggestions.is_empty() {
            command_line::draw_history_suggestions(frame, root[1], &suggestions, app.command_line_suggestion_selected, &theme);
        }
    }

    // The real terminal cursor sits right after the typed text, same
    // mechanism already used for the editor's cursor (see
    // editor.rs::cursor_screen_position) -- only while nothing else is
    // drawn over the command line (a popup below takes visual priority,
    // and moving the cursor under it would be misleading). `CommandHistory`
    // is the one exception: its popup filters live against this same
    // command line rather than owning a text field of its own (Far
    // Manager's own `Alt+F8` behaves the same way), so the cursor
    // still belongs down here, visible under the popup.
    if matches!(app.mode, Mode::Browsing | Mode::CommandHistory(_)) {
        frame.set_cursor_position(Position {
            x: root[1].x + prefix_len + app.command_line_cursor as u16,
            y: root[1].y,
        });
    }

    // The F9/Ctrl+P popups show over the browser, like a Far Manager
    // menu, not in place of it -- unlike Editing/ConfirmDiscard above,
    // which replace the whole screen.
    match &app.mode {
        Mode::MainMenu(state) => menu::draw_main_menu(frame, area, state, &theme),
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
            command_line::draw_command_history(frame, area, menu, &app.command_history, &app.command_line, &theme);
        }
        Mode::ChangeDrive(menu) => drive_menu::draw_drive_menu(frame, area, menu, &theme),
        _ => {}
    }

    [left_columns, right_columns]
}


/// Renders one panel (border, path title, footer) and its column-major
/// file grid. Returns the `(columns, visible_rows)` actually used, so
/// the caller can feed both back into `Panel::set_columns`/
/// `set_visible_rows` — `visible_rows` is just `inner.height`, the same
/// number of text rows `draw_entry_grid` itself renders into for every
/// column (they're vertical slices of one shared height).
fn draw_panel(frame: &mut Frame, area: Rect, panel: &Panel, is_active: bool, theme: &Theme) -> (usize, usize) {
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
fn build_list_item(entry: &Entry, is_selected: bool, is_marked: bool, panel_active: bool, theme: &Theme) -> ListItem<'static> {
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
        Span::styled("Ctrl+F ", Style::default().fg(theme.accent)),
        Span::styled("Find   ", Style::default().fg(theme.text_dim)),
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
/// Renders `"{cwd}> {typed}"`, matching real Far Manager's own command
/// line (which always shows the active panel's path, not just a bare
/// `>` prompt with no indication of where a command would actually
/// run). Returns the prefix's character count — `ui::draw` needs it to
/// place the real terminal cursor right after the typed text, since
/// that position now depends on `cwd`'s length, not a fixed `"> "`.
///
/// No shell-profile-name hint on the right edge anymore (an earlier
/// version had one, `"Ctrl+P {shell_name}"`) — reported as visual
/// clutter that doesn't belong on a Far-style command line; `Ctrl+P`'s
/// own picker already shows which profile is active when opened.
fn draw_command_line(frame: &mut Frame, area: Rect, cwd: &Path, command_line: &str, selection_anchor: Option<usize>, cursor: usize, theme: &Theme) -> u16 {
    let prefix = format!("{}> ", cwd.display());

    let mut spans = vec![Span::styled(prefix.clone(), Style::default().fg(theme.command_line_prefix).add_modifier(Modifier::BOLD))];

    // A `Shift`/`Ctrl+Shift`+`Left`/`Right` selection (`command_line/
    // browsing.rs`) highlights the same way the Copy/Move destination
    // field's own selection does (`text_field.rs::destination_line`,
    // `theme.current_row_bg`) -- no selection just renders as one plain
    // span, same as before this feature existed.
    match selection_anchor {
        Some(anchor) => {
            let (start, end) = crate::text_field::selection_range(anchor, cursor);
            let chars: Vec<char> = command_line.chars().collect();
            let before: String = chars[..start].iter().collect();
            let selected: String = chars[start..end].iter().collect();
            let after: String = chars[end..].iter().collect();
            spans.push(Span::styled(before, Style::default().fg(theme.text)));
            spans.push(Span::styled(selected, Style::default().fg(theme.text).bg(theme.current_row_bg)));
            spans.push(Span::styled(after, Style::default().fg(theme.text)));
        }
        None => spans.push(Span::styled(command_line.to_string(), Style::default().fg(theme.text))),
    }

    frame.render_widget(Line::from(spans), area);
    prefix.chars().count() as u16
}


/// The default F-key row, and the row shown while `Alt` is held
/// (`App::alt_held`). `F1`, `F2`, `F7`, and `F8` actually change
/// binding (`Alt+F1`/`Alt+F2` open the left/right "change drive"
/// popup, `Alt+F7` opens Find file, `Alt+F8` opens History — all four
/// are `command_line/browsing.rs`'s own raw-modifier special cases,
/// same reasoning as `Shift+F6`) — the rest keep their default action
/// and are just relabeled here to match, since Far Manager's real Alt
/// row doesn't rebind them either.
///
/// Laid out in 10 fixed-width columns spanning the full row width
/// (`Constraint::Ratio(1, 10)` each), rather than one flowing `Line`
/// with each label's own natural width — the column boundaries depend
/// only on `area`'s width, never on which label set is showing, so
/// switching between the default and `Alt` labels (`"Folder"` vs.
/// `"Find"`, six characters vs. four) can't shift anything to the
/// right of the changed column. An earlier version used a single
/// flowing line, which visibly reflowed every key from F7 onward the
/// instant `Alt` was pressed or released — reported as jarring; see
/// `tests::switching_alt_labels_does_not_shift_later_columns`, which
/// checks this against a real rendered buffer, not just the layout
/// math.
fn draw_function_keys(frame: &mut Frame, area: Rect, theme: &Theme, alt: bool) {
    let labels = if alt { &ALT_LABELS } else { &DEFAULT_LABELS };

    for (column, &(key, label)) in function_key_columns(area).into_iter().zip(labels.iter()) {
        let line = Line::from(vec![
            Span::styled(format!("{key} "), Style::default().fg(theme.accent)),
            Span::styled(label, Style::default().fg(theme.text_dim)),
        ]);
        frame.render_widget(line, column);
    }
}

/// Keep every label at 6 characters or fewer — Far Manager's own
/// convention for this row (`"UserMn"`, `"MkFold"`, `"ConfMn"`, ...),
/// and not just cosmetic: with 10 equal-width columns spanning the
/// terminal, a longer label eats into (or overruns, at narrow widths)
/// the next column's space, since there's no gap reserved between
/// columns — found by hand as `"Bookmarks"` (9 characters) visibly
/// running into `"F3 View"` with no space between them. Enforced by
/// `tests::labels_stay_within_the_six_character_budget`, not just left
/// as a comment to remember.
const DEFAULT_LABELS: [(&str, &str); 10] = [
    ("F1", "Help"), ("F2", "Menu"), ("F3", "View"), ("F4", "Edit"),
    ("F5", "Copy"), ("F6", "RenMov"), ("F7", "Folder"), ("F8", "Delete"),
    ("F9", "Menu"), ("F10", "Quit"),
];
const ALT_LABELS: [(&str, &str); 10] = [
    ("F1", "DscLft"), ("F2", "DscRht"), ("F3", "View"), ("F4", "Edit"),
    ("F5", "Copy"), ("F6", "RenMov"), ("F7", "Find"), ("F8", "Histry"),
    ("F9", "Menu"), ("F10", "Quit"),
];

/// The 10 equal-width column rects the F-key row is split into —
/// depends only on `area`, never on the labels drawn inside it.
fn function_key_columns(area: Rect) -> [Rect; 10] {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1, 10); 10])
        .split(area);
    std::array::from_fn(|i| columns[i])
}


#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, layout::Rect, Terminal};

    use super::*;

    /// Reads back the visible text of row `y`, column start inclusive,
    /// by concatenating each cell's symbol — the same shape a real
    /// terminal would show, so an `x` found in this string lines up
    /// with an actual on-screen column.
    fn rendered_row(terminal: &Terminal<TestBackend>, y: u16) -> String {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect()
    }

    fn render_function_keys(width: u16, alt: bool) -> String {
        let backend = TestBackend::new(width, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        terminal
            .draw(|frame| draw_function_keys(frame, Rect::new(0, 0, width, 1), &theme, alt))
            .unwrap();
        rendered_row(&terminal, 0)
    }

    /// The actual reported bug: the row used to be one flowing `Line`
    /// with each label's own natural width, so switching `Alt` (e.g.
    /// `"Folder"` <-> `"Find"`, six characters vs. four) visibly
    /// reflowed every key from F7 onward. Column layout fixes this —
    /// this test renders both label sets into a real buffer and checks
    /// that every later key's own column (found by its own `"F<n> "`
    /// prefix) starts at the exact same `x` regardless of which set is
    /// showing, not just that the layout math looks right on paper.
    #[test]
    fn switching_alt_labels_does_not_shift_later_columns() {
        let default_row = render_function_keys(120, false);
        let alt_row = render_function_keys(120, true);

        for key in ["F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10"] {
            let default_pos = default_row.find(key).unwrap_or_else(|| panic!("{key} missing from default row: {default_row:?}"));
            let alt_pos = alt_row.find(key).unwrap_or_else(|| panic!("{key} missing from alt row: {alt_row:?}"));
            assert_eq!(default_pos, alt_pos, "{key} shifted position when switching labels (default row: {default_row:?}, alt row: {alt_row:?})");
        }
    }

    /// The row should use the full width available to it (10 equal
    /// columns spanning `area`), not just as much as the longest
    /// label set happens to need — `function_key_columns`'s last
    /// column should reach the right edge.
    #[test]
    fn columns_span_the_full_available_width() {
        let columns = function_key_columns(Rect::new(0, 0, 100, 1));
        let last = columns.last().unwrap();
        assert_eq!(last.x + last.width, 100, "last column should reach the right edge: {columns:?}");
    }

    /// Regression test for a real reported bug: `"Bookmarks"` (9
    /// characters) visibly ran into the next column with no gap, since
    /// equal-width columns reserve no padding between them. Caps every
    /// label (both label sets) at Far Manager's own 6-character
    /// convention (`"UserMn"`, `"MkFold"`, `"ConfMn"`, ...) so this
    /// can't silently regress if a label is ever lengthened.
    #[test]
    fn labels_stay_within_the_six_character_budget() {
        for &(key, label) in DEFAULT_LABELS.iter().chain(ALT_LABELS.iter()) {
            assert!(label.len() <= 6, "{key}'s label {label:?} is {} characters, over the 6-character budget", label.len());
        }
    }
}
