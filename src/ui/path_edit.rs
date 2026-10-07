use ratatui::{
    layout::{Position, Rect},
    style::Style,
    text::Line,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use crate::path_edit::{Completions, PathEdit};
use crate::theming::Theme;

/// Rows the `Tab` list shows at most; it scrolls past that.
const MAX_LIST_ROWS: u16 = 12;

/// The path field over a pane's top border (`PathEdit`): plain text, its
/// selection styled like the panels' selected row (`selection_text`, so
/// it stays readable on a bright selection color). Scrolls sideways to keep
/// the caret in view -- it starts at the end, by the file name. Its `Tab`
/// list, if open, hangs below it. Returns the caret's cell.
pub(super) fn draw_path_field(frame: &mut Frame, area: Rect, edit: &PathEdit, theme: &Theme) -> Option<Position> {
    if area.width == 0 || area.height == 0 {
        return None;
    }
    let field = &edit.field;
    let scroll = (field.cursor() + 1).saturating_sub(usize::from(area.width));
    let style = Style::default().fg(theme.text).bg(theme.bg);
    let line = Line::from(super::text_field::styled_field_spans(field, style, super::popup::selected_row_style(theme)));
    // A `Paragraph` leaves the cells past its text alone, so the title
    // under them showed through -- the caret cell at the end repeated the
    // file name's last character.
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(line).style(style).scroll((0, scroll as u16)), area);
    if let Some(list) = &edit.completions {
        draw_completions(frame, area, list, theme);
    }
    Some(Position::new(area.x + (field.cursor() - scroll) as u16, area.y))
}


/// The `Tab` list under the field -- or over it, when there's more room
/// there (the command line, at the screen's bottom) -- as wide as its
/// longest name (within the field's width).
fn draw_completions(frame: &mut Frame, field: Rect, list: &Completions, theme: &Theme) {
    let screen = frame.area();
    let wanted = (list.items.len().max(1) as u16 + 2).min(MAX_LIST_ROWS + 2);
    let room_below = screen.bottom().saturating_sub(field.y + 1);
    let room_above = field.y.saturating_sub(screen.y);
    let below = room_below >= wanted || room_below >= room_above;
    let height = wanted.min(if below { room_below } else { room_above });
    let top = if below { field.y + 1 } else { field.y - height };
    let longest = list.items.iter().map(|(name, is_dir)| name.chars().count() + usize::from(*is_dir)).max().unwrap_or(0);
    let width = (longest as u16 + 2).max(20).min(field.width);
    if height < 3 || width < 3 {
        return;
    }
    let area = Rect::new(field.x, top, width, height);
    let rows = usize::from(height - 2);
    let first = (list.selected + 1).saturating_sub(rows);
    let mut items: Vec<ListItem> = list
        .items
        .iter()
        .enumerate()
        .skip(first)
        .take(rows)
        .map(|(index, (name, is_dir))| {
            let label = if *is_dir { format!("{name}{}", std::path::MAIN_SEPARATOR) } else { name.clone() };
            let style = if index == list.selected { super::popup::selected_row_style(theme) } else { Style::default().fg(theme.text) };
            ListItem::new(Line::styled(label, style))
        })
        .collect();
    if items.is_empty() {
        items.push(ListItem::new(Line::styled("no matches", Style::default().fg(theme.text_dim))));
    }
    let block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme.accent));
    frame.render_widget(Clear, area);
    frame.render_widget(List::new(items).block(block), area);
}


#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;
    use crate::test_support::{key, unique_scratch_dir};

    #[test]
    fn the_tab_list_hangs_under_the_field_with_the_selection() {
        let dir = unique_scratch_dir("path-edit-draw");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("setup.txt"), "").unwrap();
        let mut edit = PathEdit::new(&dir);
        edit.field.set_text("s");
        edit.key(key(KeyCode::Tab));
        edit.key(key(KeyCode::Down));

        let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
        let theme = Theme::dark();
        terminal.draw(|frame| {
            draw_path_field(frame, Rect::new(1, 0, 38, 1), &edit, &theme);
        }).unwrap();

        let buffer = terminal.backend().buffer();
        let row = |y: u16| (0..40).map(|x| buffer[(x, y)].symbol()).collect::<String>();
        assert!(row(2).contains(&format!("src{}", std::path::MAIN_SEPARATOR)), "directories first: {:?}", row(2));
        assert!(row(3).contains("setup.txt"));
        assert_eq!(buffer[(2, 3)].bg, super::super::popup::selected_row_style(&theme).bg.unwrap(), "the highlighted row is the second");
    }

    #[test]
    fn an_empty_list_says_no_matches() {
        let mut edit = PathEdit::new(&unique_scratch_dir("path-edit-draw"));
        edit.field.set_text("zzz");
        edit.key(key(KeyCode::Tab));

        let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
        terminal.draw(|frame| {
            draw_path_field(frame, Rect::new(1, 0, 38, 1), &edit, &Theme::dark());
        }).unwrap();

        let buffer = terminal.backend().buffer();
        let row: String = (0..40).map(|x| buffer[(x, 2)].symbol()).collect();
        assert!(row.contains("no matches"), "{row:?}");
    }

    /// The command line sits on the screen's last row: its list opens
    /// upwards.
    #[test]
    fn with_no_room_below_the_list_opens_above() {
        let dir = unique_scratch_dir("path-edit-draw");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("scripts")).unwrap();
        let mut edit = PathEdit::new(&dir);
        edit.field.set_text("s");
        edit.key(key(KeyCode::Tab));

        let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
        terminal.draw(|frame| {
            draw_path_field(frame, Rect::new(0, 7, 40, 1), &edit, &Theme::dark());
        }).unwrap();

        let buffer = terminal.backend().buffer();
        let rows: Vec<String> = (0..8).map(|y| (0..40).map(|x| buffer[(x, y)].symbol()).collect()).collect();
        assert!(rows[4].contains("scripts") && rows[5].contains("src"), "{rows:#?}");
    }
}
