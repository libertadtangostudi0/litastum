use ratatui::{
    layout::{Constraint, Layout, Position, Rect},
    text::Line,
    widgets::Paragraph,
    Frame,
};

use crate::app::App;
use crate::user_screen::LiveView;

/// Rows the user screen gets on a terminal `height` rows tall: all but the
/// command line and the key bar under it.
pub fn console_rows(height: u16) -> u16 {
    height.saturating_sub(2).max(1)
}


/// The user screen, as Far's: what commands printed (`App::user_screen`),
/// a running command's output live under it, and our own command line and
/// key bar below -- `Ctrl+O`, and while a command runs. The cursor is the
/// running program's, else the command line's.
pub fn draw_console(frame: &mut Frame, app: &App, live: Option<&LiveView>) -> Option<Position> {
    let theme = app.theme;
    let [screen, line_row, keys_row] = Layout::vertical([Constraint::Min(1), Constraint::Length(1), Constraint::Length(1)]).areas(frame.area());
    let screen_cursor = draw_screen_lines(frame, screen, app, live);
    let command_cursor = super::draw_command_rows(frame, line_row, keys_row, app, &theme);
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
    let lines: Vec<Line> = (start..end).map(|index| if index < kept.len() { kept[index].clone() } else { running[index - kept.len()].clone() }).collect();
    frame.render_widget(Paragraph::new(lines), area);

    let (row, column) = live?.cursor?;
    let index = kept.len() + row;
    (start..end).contains(&index).then(|| at(index - start, column))
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

        let (rows, cursor) = rendered(&app, Some(&live), 6);

        assert_eq!(rows[..3], ["C:> dir", "a.txt", "running"]);
        assert_eq!(cursor, Some(Position::new(7, 2)), "the program's cursor, not the command line's");
        assert!(rows[4].starts_with(&dir.to_string_lossy()[..3]), "the command line under the screen: {:?}", rows[4]);
    }

    #[test]
    fn more_lines_than_fit_show_the_last_ones_unless_scrolled() {
        let mut app = test_app(unique_scratch_dir("console-view"));
        app.user_screen.extend((1..=10).map(|n| Line::raw(n.to_string())).collect());

        let (rows, cursor) = rendered(&app, None, 5);
        assert_eq!(rows[..3], ["8", "9", "10"]);
        assert_eq!(cursor.map(|position| position.y), Some(3), "the command line's cursor");

        app.user_screen.scroll_by(2, 3);
        assert_eq!(rendered(&app, None, 5).0[..3], ["6", "7", "8"]);
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
}
