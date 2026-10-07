use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line as GridLine};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// The last `count` lines of what a command printed so far -- scrollback
/// then the screen down to its last used row (the cursor's, or the last
/// with text) -- and where the cursor is among them, if it is.
pub(super) fn tail_lines<T>(term: &Term<T>, count: usize) -> (Vec<Line<'static>>, Option<(usize, u16)>) {
    let grid = term.grid();
    let history = grid.history_size();
    let cursor = grid.cursor.point;
    let cursor_row = cursor.line.0.max(0) as usize;
    let last_text_row = (0..grid.screen_lines()).rev().find(|&row| !row_is_blank(&grid[GridLine(row as i32)], grid.columns()));
    let used_rows = last_text_row.map_or(0, |row| row + 1).max(cursor_row + 1);

    let total = history + used_rows;
    let first = total.saturating_sub(count);
    let lines = (first..total).map(|index| row_line(&grid[GridLine(index as i32 - history as i32)], grid.columns())).collect();
    let cursor_index = history + cursor_row;
    let cursor = (cursor_index >= first).then(|| (cursor_index - first, cursor.column.0 as u16));
    (lines, cursor)
}


/// The cursor line's text up to the cursor.
pub(super) fn text_before_cursor<T>(term: &Term<T>) -> String {
    let grid = term.grid();
    let point = grid.cursor.point;
    let row = row_line(&grid[point.line], grid.columns()).to_string();
    row.chars().take(point.column.0).collect()
}


/// The screen itself, every row -- for a program on the alternate screen
/// (an editor, a pager), which draws the whole of it.
pub(super) fn screen_lines<T>(term: &Term<T>) -> Vec<Line<'static>> {
    let grid = term.grid();
    (0..grid.screen_lines()).map(|row| row_line(&grid[GridLine(row as i32)], grid.columns())).collect()
}


/// Everything a finished command printed: scrollback and screen, without
/// the blank rows after its last text.
pub(super) fn all_lines<T>(term: &Term<T>) -> Vec<Line<'static>> {
    let grid = term.grid();
    let mut lines = tail_lines(term, grid.history_size() + grid.screen_lines()).0;
    while lines.last().is_some_and(|line| line.spans.iter().all(|span| span.content.trim().is_empty())) {
        lines.pop();
    }
    lines
}


fn row_is_blank(row: &alacritty_terminal::grid::Row<Cell>, columns: usize) -> bool {
    (0..columns).all(|column| {
        let cell = &row[Column(column)];
        cell.c == ' ' && cell.bg == AnsiColor::Named(NamedColor::Background)
    })
}


/// One grid row as spans of one style each, trailing blanks dropped.
fn row_line(row: &alacritty_terminal::grid::Row<Cell>, columns: usize) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut text = String::new();
    let mut style = Style::default();
    let mut blank_tail = 0;
    for column in 0..columns {
        let cell = &row[Column(column)];
        if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
            continue;
        }
        let cell_style = cell_style(cell);
        if cell_style != style && !text.is_empty() {
            spans.push(Span::styled(std::mem::take(&mut text), style));
            blank_tail = 0;
        }
        style = cell_style;
        text.push(cell.c);
        if let Some(extra) = cell.zerowidth() {
            text.extend(extra);
        }
        blank_tail = if cell.c == ' ' && style.bg.is_none() { blank_tail + 1 } else { 0 };
    }
    text.truncate(text.len() - blank_tail.min(text.len()));
    if !text.is_empty() {
        spans.push(Span::styled(text, style));
    }
    Line::from(spans)
}


fn cell_style(cell: &Cell) -> Style {
    let mut style = Style::default();
    let mut modifier = Modifier::empty();
    if let Some(fg) = color(cell.fg, &mut modifier) {
        style = style.fg(fg);
    }
    if let Some(bg) = color(cell.bg, &mut Modifier::empty()) {
        style = style.bg(bg);
    }
    for (flag, add) in [
        (Flags::BOLD, Modifier::BOLD),
        (Flags::ITALIC, Modifier::ITALIC),
        (Flags::ALL_UNDERLINES, Modifier::UNDERLINED),
        (Flags::DIM, Modifier::DIM),
        (Flags::INVERSE, Modifier::REVERSED),
        (Flags::HIDDEN, Modifier::HIDDEN),
        (Flags::STRIKEOUT, Modifier::CROSSED_OUT),
    ] {
        if cell.flags.intersects(flag) {
            modifier |= add;
        }
    }
    style.add_modifier(modifier)
}


/// A cell color for the real terminal: the 16 ANSI colors by index, so
/// they look as they would printed directly (the terminal's own palette);
/// the default colors as the terminal's default (`None`), which follows
/// the theme (`terminal_palette`). A dim named color is its base, dimmed.
fn color(color: AnsiColor, modifier: &mut Modifier) -> Option<Color> {
    match color {
        AnsiColor::Spec(rgb) => Some(Color::Rgb(rgb.r, rgb.g, rgb.b)),
        AnsiColor::Indexed(index) => Some(Color::Indexed(index)),
        AnsiColor::Named(named) => {
            let index = named as usize;
            if index < 16 {
                Some(Color::Indexed(index as u8))
            } else if (NamedColor::DimBlack as usize..=NamedColor::DimWhite as usize).contains(&index) {
                *modifier |= Modifier::DIM;
                Some(Color::Indexed((index - NamedColor::DimBlack as usize) as u8))
            } else {
                None
            }
        }
    }
}


#[cfg(test)]
pub(super) mod tests {
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::vte::ansi::Processor;

    use super::*;
    use crate::user_screen::session::GridSize;

    pub(crate) fn term_with(columns: usize, lines: usize, output: &[u8]) -> Term<VoidListener> {
        let mut term = Term::new(Config::default(), &GridSize { columns, lines }, VoidListener);
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, output);
        term
    }

    fn texts(lines: &[Line]) -> Vec<String> {
        lines.iter().map(|line| line.to_string()).collect()
    }

    #[test]
    fn printed_text_comes_back_as_lines_with_its_colors() {
        let term = term_with(20, 5, b"\x1b[31mred\x1b[0m plain\r\nnext");

        let (lines, cursor) = tail_lines(&term, 10);

        assert_eq!(texts(&lines), ["red plain", "next"], "down to the cursor's row, trailing blanks dropped");
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Indexed(1)));
        assert_eq!(lines[0].spans[1].style.fg, None, "the default color stays the terminal's");
        assert_eq!(cursor, Some((1, 4)));
    }

    #[test]
    fn the_tail_includes_scrollback_and_is_cut_from_the_top() {
        let term = term_with(10, 3, b"1\r\n2\r\n3\r\n4\r\n5");

        assert_eq!(texts(&tail_lines(&term, 10).0), ["1", "2", "3", "4", "5"], "scrolled-off lines first");
        let (tail, cursor) = tail_lines(&term, 2);
        assert_eq!(texts(&tail), ["4", "5"]);
        assert_eq!(cursor, Some((1, 1)));
    }

    #[test]
    fn a_finished_command_drops_trailing_blank_rows() {
        let term = term_with(10, 5, b"done\r\n\r\n");
        assert_eq!(texts(&all_lines(&term)), ["done"]);
    }

    #[test]
    fn wide_characters_are_one_character() {
        let term = term_with(10, 2, "日本".as_bytes());
        assert_eq!(texts(&tail_lines(&term, 1).0), ["日本"]);
    }
}
