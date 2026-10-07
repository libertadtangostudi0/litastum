use std::time::{Duration, Instant};

use super::UserScreen;

/// A place on the user screen: a kept line and a character column.
pub type Point = (usize, usize);

/// Text selected with the mouse on the user screen: from where the button
/// went down (`anchor`) to where it is (`end`), both included. A click
/// alone leaves an empty one -- just the anchor, for a `Shift`/`Ctrl`
/// click to extend from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Selection {
    anchor: Point,
    end: Point,
    /// The button is still down.
    dragging: bool,
}

/// Clicks closer together than this, on the same place, are one double
/// click (Windows' own default).
const MULTI_CLICK: Duration = Duration::from_millis(500);


/// A click: when, where, whether it was the second of a double click, and
/// whether that word was already selected when it began -- a double
/// click there takes the line.
#[derive(Debug, Clone, Copy)]
pub(super) struct Click {
    at: Instant,
    point: Point,
    double: bool,
    on_selected_word: bool,
}


/// Characters that end a word for a double click besides blanks: a path
/// or a URL stays whole, the quotes and brackets around it don't.
fn ends_word(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | ',' | ';' | '|')
}


impl UserScreen {
    /// A left click at `row`, `column` of the view, at `at`: a double
    /// click selects the word there, and another double click on that
    /// selected word the line (requested) -- then `Ctrl+C` copies; a
    /// single click is `select_start`.
    pub fn click(&mut self, row: u16, column: u16, extend: bool, at: Instant) {
        let Some(point) = self.point_at(row, column) else {
            return;
        };
        let double = self.last_click.is_some_and(|last| !extend && !last.double && last.point == point && at.duration_since(last.at) <= MULTI_CLICK);
        let on_selected_word = match self.last_click {
            Some(last) if double => last.on_selected_word,
            // Checked before this click's anchor replaces the selection.
            _ => self.word_span(point).is_some_and(|span| self.selection() == Some(span)),
        };
        self.last_click = Some(Click { at, point, double, on_selected_word });
        if !double {
            self.select_start(row, column, extend);
        } else if on_selected_word {
            self.select_line(point.0);
        } else if let Some((anchor, end)) = self.word_span(point) {
            self.selection = Some(Selection { anchor, end, dragging: false });
        }
    }

    /// The word around `point`, first to last character; `None` on a
    /// blank or past the text.
    fn word_span(&self, (line, column): Point) -> Option<(Point, Point)> {
        let chars: Vec<char> = self.lines.get(line)?.to_string().chars().collect();
        if chars.get(column).is_none_or(|&c| ends_word(c)) {
            return None;
        }
        let start = chars[..column].iter().rposition(|&c| ends_word(c)).map_or(0, |i| i + 1);
        let end = chars[column..].iter().position(|&c| ends_word(c)).map_or(chars.len(), |i| column + i) - 1;
        Some(((line, start), (line, end)))
    }

    fn select_line(&mut self, line: usize) {
        let length = self.lines[line].to_string().trim_end().chars().count();
        if length > 0 {
            self.selection = Some(Selection { anchor: (line, 0), end: (line, length - 1), dragging: false });
        }
    }

    /// The first kept line on view, as the renderer lays them out: the
    /// last `visible_rows` lines, less `scroll`.
    pub fn first_visible_line(&self) -> usize {
        self.lines.len().saturating_sub(self.scroll).saturating_sub(self.visible_rows())
    }

    /// The selection, start before end; `None` with nothing selected (an
    /// anchor alone selects nothing).
    pub fn selection(&self) -> Option<(Point, Point)> {
        let selection = self.selection.filter(|selection| selection.anchor != selection.end)?;
        Some(if selection.anchor <= selection.end { (selection.anchor, selection.end) } else { (selection.end, selection.anchor) })
    }

    /// The left button went down at `row`, `column` of the view: a new
    /// anchor, or with `extend` (`Shift`/`Ctrl` held) the selection from
    /// the anchor so far to here -- click, scroll, extend-click, as in an
    /// editor. Reported: a selection couldn't reach past the view with a
    /// touchpad, where holding the button while scrolling is awkward.
    pub fn select_start(&mut self, row: u16, column: u16, extend: bool) {
        let Some(point) = self.point_at(row, column) else {
            return;
        };
        match self.selection.as_mut() {
            Some(selection) if extend => {
                selection.end = point;
                selection.dragging = true;
            }
            _ => self.selection = Some(Selection { anchor: point, end: point, dragging: true }),
        }
    }

    /// The mouse moved with the button down: the selection follows, and at
    /// the view's top or bottom row the text scrolls on -- reported: text
    /// longer than a page couldn't be selected.
    pub fn select_to(&mut self, row: u16, column: u16) {
        if !self.selection.is_some_and(|selection| selection.dragging) {
            return;
        }
        let rows = self.visible_rows();
        if row == 0 {
            self.scroll_by(1, rows);
        } else if usize::from(row) + 1 >= rows {
            self.scroll_by(-1, rows);
        }
        let row = row.min(rows.saturating_sub(1) as u16);
        if let (Some(point), Some(selection)) = (self.point_at(row, column), self.selection.as_mut()) {
            selection.end = point;
        }
    }

    /// The wheel turned: during a drag the selection's end moves with the
    /// text under the pointer.
    pub fn scroll_selecting(&mut self, delta: isize, row: u16, column: u16) {
        let rows = self.visible_rows();
        self.scroll_by(delta, rows);
        if self.selection.is_some_and(|selection| selection.dragging) {
            if let (Some(point), Some(selection)) = (self.point_at(row.min(rows.saturating_sub(1) as u16), column), self.selection.as_mut()) {
                selection.end = point;
            }
        }
    }

    /// The button came up. A click without a drag selects nothing, but
    /// its anchor stays.
    pub fn select_finish(&mut self) {
        if let Some(selection) = self.selection.as_mut() {
            selection.dragging = false;
        }
    }

    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    /// The selected text, a line per kept line, trailing blanks dropped.
    pub fn selected_text(&self) -> Option<String> {
        let ((first_line, first_column), (last_line, last_column)) = self.selection()?;
        let lines: Vec<String> = (first_line..=last_line.min(self.lines.len().saturating_sub(1)))
            .map(|index| {
                let text = self.lines[index].to_string();
                let from = if index == first_line { first_column } else { 0 };
                let to = if index == last_line { last_column + 1 } else { usize::MAX };
                text.chars().skip(from).take(to.saturating_sub(from)).collect::<String>().trim_end().to_string()
            })
            .collect();
        Some(lines.join("\n"))
    }

    fn point_at(&self, row: u16, column: u16) -> Option<Point> {
        let line = self.first_visible_line() + usize::from(row);
        (usize::from(row) < self.visible_rows() && line < self.lines.len()).then_some((line, usize::from(column)))
    }
}


#[cfg(test)]
mod tests {
    use ratatui::text::Line;

    use super::*;

    fn screen(lines: usize, rows: usize) -> UserScreen {
        let mut screen = UserScreen::default();
        screen.extend((0..lines).map(|n| Line::raw(format!("line {n}"))).collect());
        screen.set_visible_rows(rows);
        screen
    }

    #[test]
    fn a_drag_selects_across_lines() {
        let mut screen = screen(5, 5);
        screen.select_start(1, 5, false);
        screen.select_to(2, 3);
        screen.select_finish();

        assert_eq!(screen.selected_text().as_deref(), Some("1\nline"));
    }

    #[test]
    fn a_drag_upwards_selects_the_same_way() {
        let mut screen = screen(5, 5);
        screen.select_start(2, 3, false);
        screen.select_to(1, 5);

        assert_eq!(screen.selected_text().as_deref(), Some("1\nline"));
    }

    /// Reported: text longer than a page couldn't be selected.
    #[test]
    fn dragging_at_the_top_row_scrolls_back_and_selects_on() {
        let mut screen = screen(20, 5);
        assert_eq!(screen.first_visible_line(), 15);
        screen.select_start(4, 0, false);

        for _ in 0..3 {
            screen.select_to(0, 0);
        }

        assert_eq!(screen.first_visible_line(), 12, "scrolled up a line per move at the edge");
        let text = screen.selected_text().unwrap();
        assert!(text.starts_with("line 12") && text.ends_with('l'), "{text:?}");
    }

    #[test]
    fn the_wheel_during_a_drag_moves_the_end() {
        let mut screen = screen(20, 5);
        screen.select_start(4, 6, false);
        screen.scroll_selecting(10, 0, 0);

        assert_eq!(screen.selection(), Some(((5, 0), (19, 6))));
    }

    #[test]
    fn a_click_selects_nothing() {
        let mut screen = screen(5, 5);
        screen.select_start(1, 1, false);
        screen.select_finish();

        assert_eq!(screen.selection(), None);
    }

    /// Reported: with a touchpad, a selection couldn't reach past the
    /// view. Click the start, scroll, `Shift`-click the end.
    #[test]
    fn a_click_then_an_extending_click_after_scrolling_selects_between() {
        let mut screen = screen(20, 5);
        screen.select_start(4, 0, false);
        screen.select_finish();
        assert_eq!(screen.selection(), None, "the click alone selects nothing");

        screen.scroll_by(10, 5);
        screen.select_start(0, 3, true);
        screen.select_finish();

        assert_eq!(screen.selection(), Some(((5, 3), (19, 0))));
        assert!(screen.selected_text().unwrap().starts_with("e 5"));
    }

    #[test]
    fn an_extending_click_without_an_anchor_starts_one() {
        let mut screen = screen(5, 5);
        screen.select_start(2, 1, true);
        screen.select_finish();
        screen.select_start(3, 2, true);

        assert_eq!(screen.selection(), Some(((2, 1), (3, 2))));
    }

    /// Requested: a double click selects the word, another double click
    /// on it the line.
    #[test]
    fn a_double_click_selects_the_word_and_another_one_the_line() {
        let mut screen = UserScreen::default();
        screen.extend(vec![Line::raw("svn commit -F \"W:\\x\\cmt_msg.txt\" RFI14.1")]);
        screen.set_visible_rows(5);
        let start = Instant::now();
        let double_click = |screen: &mut UserScreen, ms: u64| {
            screen.click(0, 20, false, start + Duration::from_millis(ms));
            screen.select_finish();
            screen.click(0, 20, false, start + Duration::from_millis(ms + 150));
            screen.select_finish();
        };

        screen.click(0, 20, false, start);
        screen.select_finish();
        assert_eq!(screen.selected_text(), None, "one click: just the anchor");

        double_click(&mut screen, 1000);
        assert_eq!(screen.selected_text().as_deref(), Some("W:\\x\\cmt_msg.txt"), "a path stays whole, without its quotes");
        double_click(&mut screen, 2000);
        assert_eq!(screen.selected_text().as_deref(), Some("svn commit -F \"W:\\x\\cmt_msg.txt\" RFI14.1"), "again on the selected word: the line");
        double_click(&mut screen, 3000);
        assert_eq!(screen.selected_text().as_deref(), Some("W:\\x\\cmt_msg.txt"), "and the word again");

        screen.clear_selection();
        double_click(&mut screen, 4000);
        assert_eq!(screen.selected_text().as_deref(), Some("W:\\x\\cmt_msg.txt"), "copied and cleared: a word again, not the line");
    }

    #[test]
    fn a_third_quick_click_is_a_single_one() {
        let mut screen = UserScreen::default();
        screen.extend(vec![Line::raw("alpha beta")]);
        screen.set_visible_rows(5);
        let start = Instant::now();

        screen.click(0, 1, false, start);
        screen.click(0, 1, false, start + Duration::from_millis(100));
        assert_eq!(screen.selected_text().as_deref(), Some("alpha"));
        screen.click(0, 1, false, start + Duration::from_millis(200));
        assert_eq!(screen.selected_text(), None, "no triple click: just an anchor");
    }

    #[test]
    fn slow_or_moved_clicks_are_single_ones() {
        let mut screen = UserScreen::default();
        screen.extend(vec![Line::raw("alpha beta")]);
        screen.set_visible_rows(5);
        let start = Instant::now();

        screen.click(0, 1, false, start);
        screen.click(0, 1, false, start + Duration::from_millis(900));
        assert_eq!(screen.selected_text(), None, "too slow");
        screen.click(0, 7, false, start + Duration::from_millis(1000));
        assert_eq!(screen.selected_text(), None, "somewhere else");
        screen.click(0, 7, false, start + Duration::from_millis(1100));
        assert_eq!(screen.selected_text().as_deref(), Some("beta"));
    }

    #[test]
    fn a_double_click_on_a_blank_selects_nothing() {
        let mut screen = UserScreen::default();
        screen.extend(vec![Line::raw("a  b")]);
        screen.set_visible_rows(5);
        let start = Instant::now();

        screen.click(0, 1, false, start);
        screen.click(0, 1, false, start + Duration::from_millis(100));

        assert_eq!(screen.selected_text(), None);
    }
}
