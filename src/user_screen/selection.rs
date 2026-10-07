use super::UserScreen;

/// A place on the user screen: a kept line and a character column.
pub type Point = (usize, usize);

/// Text selected with the mouse on the user screen: from where the button
/// went down (`anchor`) to where it is (`end`), both included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Selection {
    anchor: Point,
    end: Point,
    /// The button is still down.
    dragging: bool,
}

impl UserScreen {
    /// The first kept line on view, as the renderer lays them out: the
    /// last `visible_rows` lines, less `scroll`.
    pub fn first_visible_line(&self) -> usize {
        self.lines.len().saturating_sub(self.scroll).saturating_sub(self.visible_rows())
    }

    /// The selection, start before end; `None` with nothing selected.
    pub fn selection(&self) -> Option<(Point, Point)> {
        let selection = self.selection?;
        Some(if selection.anchor <= selection.end { (selection.anchor, selection.end) } else { (selection.end, selection.anchor) })
    }

    /// The left button went down at `row`, `column` of the view.
    pub fn select_start(&mut self, row: u16, column: u16) {
        self.selection = self.point_at(row, column).map(|point| Selection { anchor: point, end: point, dragging: true });
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

    /// The button came up: a click without a drag selects nothing.
    pub fn select_finish(&mut self) {
        match self.selection.as_mut() {
            Some(selection) if selection.anchor == selection.end => self.selection = None,
            Some(selection) => selection.dragging = false,
            None => {}
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
        screen.select_start(1, 5);
        screen.select_to(2, 3);
        screen.select_finish();

        assert_eq!(screen.selected_text().as_deref(), Some("1\nline"));
    }

    #[test]
    fn a_drag_upwards_selects_the_same_way() {
        let mut screen = screen(5, 5);
        screen.select_start(2, 3);
        screen.select_to(1, 5);

        assert_eq!(screen.selected_text().as_deref(), Some("1\nline"));
    }

    /// Reported: text longer than a page couldn't be selected.
    #[test]
    fn dragging_at_the_top_row_scrolls_back_and_selects_on() {
        let mut screen = screen(20, 5);
        assert_eq!(screen.first_visible_line(), 15);
        screen.select_start(4, 0);

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
        screen.select_start(4, 6);
        screen.scroll_selecting(10, 0, 0);

        assert_eq!(screen.selection(), Some(((5, 0), (19, 6))));
    }

    #[test]
    fn a_click_selects_nothing() {
        let mut screen = screen(5, 5);
        screen.select_start(1, 1);
        screen.select_finish();

        assert_eq!(screen.selection(), None);
    }
}
