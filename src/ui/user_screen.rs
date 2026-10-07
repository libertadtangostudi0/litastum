use ratatui::{
    layout::{Constraint, Layout, Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::App;
use crate::user_screen::LiveView;

/// Rows the user screen gets on a terminal `height` rows tall: all but the
/// command line under it.
pub fn console_rows(height: u16) -> u16 {
    height.saturating_sub(1).max(1)
}


/// The user screen, as Far's: what commands printed (`App::user_screen`),
/// a running command's output live under it, and our own command line
/// below -- `Ctrl+O`, and while a command runs (what's typed meanwhile
/// shows in it, without suggestions). The cursor is the running
/// program's, else the command line's.
pub fn draw_console(frame: &mut Frame, app: &App, live: Option<&LiveView>) -> Option<Position> {
    let theme = app.theme;
    let [screen, line_row] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());
    let screen_cursor = draw_screen_lines(frame, screen, app, live);
    let command_cursor = super::draw_command_rows(frame, line_row, app, &theme, live.is_none());
    match live {
        Some(_) => screen_cursor,
        None => Some(command_cursor),
    }
}


/// The kept lines, then the running command's, as a terminal shows them:
/// from the top while they fit, else the last ones (less `scroll`). A
/// full-screen program gets the area to itself.
fn draw_screen_lines(frame: &mut Frame, area: Rect, app: &App, live: Option<&LiveView>) -> Option<Position> {
    let at = |row: usize, column: u16| Position::new(area.x + column.min(area.width.saturating_sub(1)), area.y + row as u16);
    if let Some(live) = live.filter(|live| live.full_screen) {
        frame.render_widget(Paragraph::new(live.lines.clone()), area);
        return live.cursor.filter(|&(row, _)| row < usize::from(area.height)).map(|(row, column)| at(row, column));
    }

    let kept = app.user_screen.lines();
    let running: &[Line<'static>] = live.map_or(&[], |live| &live.lines);
    let total = kept.len() + running.len();
    let scroll = if live.is_some() { 0 } else { app.user_screen.scroll() };
    let end = total.saturating_sub(scroll);
    let start = end.saturating_sub(usize::from(area.height));
    let selection = app.user_screen.selection();
    let lines: Vec<Line> = (start..end)
        .map(|index| match (index < kept.len(), selection) {
            (true, Some(((first, from), (last, to)))) if (first..=last).contains(&index) => {
                let from = if index == first { from } else { 0 };
                let to = if index == last { to + 1 } else { usize::MAX };
                highlighted(&kept[index], from, to)
            }
            (true, _) => kept[index].clone(),
            (false, _) => running[index - kept.len()].clone(),
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), area);

    let (row, column) = live?.cursor?;
    let index = kept.len() + row;
    (start..end).contains(&index).then(|| at(index - start, column))
}


/// `line` with its characters `from..to` shown selected (reversed); a
/// selection past the text reaches the line's end.
fn highlighted(line: &Line<'static>, from: usize, to: usize) -> Line<'static> {
    let selected = Style::default().add_modifier(Modifier::REVERSED);
    let mut spans = Vec::new();
    let mut column = 0;
    for span in &line.spans {
        let mut part = String::new();
        let mut part_selected = false;
        for c in span.content.chars() {
            let is_selected = (from..to).contains(&column);
            if is_selected != part_selected && !part.is_empty() {
                let style = if part_selected { span.style.patch(selected) } else { span.style };
                spans.push(Span::styled(std::mem::take(&mut part), style));
            }
            part_selected = is_selected;
            part.push(c);
            column += 1;
        }
        if !part.is_empty() {
            let style = if part_selected { span.style.patch(selected) } else { span.style };
            spans.push(Span::styled(part, style));
        }
    }
    Line::from(spans)
}


#[cfg(test)]
mod tests {
    use std::path::Path;

    use ratatui::{backend::TestBackend, Terminal};

    use super::*;
    use crate::test_support::{test_app, unique_scratch_dir};

    fn rendered(app: &App, live: Option<&LiveView>, height: u16) -> (Vec<String>, Option<Position>) {
        let mut terminal = Terminal::new(TestBackend::new(30, height)).unwrap();
        let mut cursor = None;
        terminal.draw(|frame| cursor = draw_console(frame, app, live)).unwrap();
        let buffer = terminal.backend().buffer();
        let rows = (0..height).map(|y| (0..30).map(|x| buffer[(x, y)].symbol()).collect::<String>().trim_end().to_string()).collect();
        (rows, cursor)
    }

    #[test]
    fn kept_output_then_the_running_command_with_its_cursor() {
        let dir = unique_scratch_dir("console-view");
        let mut app = test_app(dir.clone());
        let theme = app.theme;
        app.user_screen.begin_command(Path::new("C:"), "dir", &theme);
        app.user_screen.extend(vec![Line::raw("a.txt")]);
        let live = LiveView { lines: vec![Line::raw("running")], cursor: Some((0, 7)), full_screen: false };

        let (rows, cursor) = rendered(&app, Some(&live), 5);

        assert_eq!(rows[..3], ["C:> dir", "a.txt", "running"]);
        assert_eq!(cursor, Some(Position::new(7, 2)), "the program's cursor, not the command line's");
        assert!(rows[4].starts_with(&dir.to_string_lossy()[..3]), "the command line under the screen: {:?}", rows[4]);
    }

    #[test]
    fn more_lines_than_fit_show_the_last_ones_unless_scrolled() {
        let mut app = test_app(unique_scratch_dir("console-view"));
        app.user_screen.extend((1..=10).map(|n| Line::raw(n.to_string())).collect());

        let (rows, cursor) = rendered(&app, None, 4);
        assert_eq!(rows[..3], ["8", "9", "10"]);
        assert_eq!(cursor.map(|position| position.y), Some(3), "the command line's cursor, on the last row");

        app.user_screen.scroll_by(2, 3);
        assert_eq!(rendered(&app, None, 4).0[..3], ["6", "7", "8"]);
    }

    #[test]
    fn a_full_screen_program_hides_the_kept_output() {
        let mut app = test_app(unique_scratch_dir("console-view"));
        app.user_screen.extend(vec![Line::raw("old")]);
        let live = LiveView { lines: vec![Line::raw("editor")], cursor: None, full_screen: true };

        let (rows, cursor) = rendered(&app, Some(&live), 5);

        assert_eq!(rows[0], "editor");
        assert_eq!(cursor, None, "the program hid its cursor");
    }

    #[test]
    fn the_selection_is_drawn_reversed() {
        let line = Line::from(vec![Span::raw("ab"), Span::styled("cd", Style::default().add_modifier(Modifier::BOLD))]);

        let drawn = highlighted(&line, 1, 3);

        let parts: Vec<(String, bool)> = drawn.spans.iter().map(|span| (span.content.to_string(), span.style.add_modifier.contains(Modifier::REVERSED))).collect();
        assert_eq!(parts, [("a".into(), false), ("b".into(), true), ("c".into(), true), ("d".into(), false)]);
    }
}
