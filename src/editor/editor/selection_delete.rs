use edtui::{EditorMode, Index2, Lines, RowIndex};

use super::Editor;

impl Editor {
    /// Deletes the active selection (`Standard` keymap), returning to
    /// typing at its start. `edtui`'s own `DeleteSelection` removes a whole
    /// row once every character on it is selected, line break included,
    /// so a paste over a selected line ran into the next one. Returns
    /// whether there was a selection. Doesn't record undo -- callers have.
    pub(super) fn delete_selection(&mut self) -> bool {
        let Some(selection) = self.state.selection.take() else {
            return false;
        };
        self.state.cursor = delete_inclusive(&mut self.state.lines, selection.start, selection.end);
        self.state.mode = EditorMode::Insert;
        true
    }
}


/// Deletes the characters from `a` to `b`, both included, in either
/// order -- `edtui`'s selection convention. `(row, len)` stands for that
/// row's line break, so a range ending there joins the next row on.
/// Returns where the range started.
fn delete_inclusive(lines: &mut Lines, a: Index2, b: Index2) -> Index2 {
    let (start, end) = if (a.row, a.col) <= (b.row, b.col) { (a, b) } else { (b, a) };
    let row_len = |lines: &Lines, row: usize| lines.len_col(row).unwrap_or(0);
    let start = Index2::new(start.row, start.col.min(row_len(lines, start.row)));

    // The first position after the range: the next row's start when the
    // range takes a line break, else the character after `end`.
    let end_len = row_len(lines, end.row);
    let (after_row, after_col) = if end.col >= end_len {
        if end.row + 1 < lines.len() { (end.row + 1, 0) } else { (end.row, end_len) }
    } else {
        (end.row, end.col + 1)
    };

    let tail: Vec<char> = lines.get(RowIndex::new(after_row)).map(|row| row[after_col.min(row.len())..].to_vec()).unwrap_or_default();
    for _ in start.row + 1..=after_row {
        lines.remove(RowIndex::new(start.row + 1));
    }
    if let Some(row) = lines.get_mut(RowIndex::new(start.row)) {
        row.truncate(start.col);
        row.extend(tail);
    }
    start
}


#[cfg(test)]
mod tests {
    use super::*;

    fn delete(text: &str, a: (usize, usize), b: (usize, usize)) -> (String, Index2) {
        let mut lines = Lines::from(text);
        let cursor = delete_inclusive(&mut lines, Index2::new(a.0, a.1), Index2::new(b.0, b.1));
        (lines.to_string(), cursor)
    }

    #[test]
    fn deletes_a_range_inside_one_line() {
        assert_eq!(delete("hello world\n", (0, 6), (0, 10)), ("hello \n".to_string(), Index2::new(0, 6)));
    }

    /// `edtui`'s own delete removed the row here, line break and all.
    #[test]
    fn a_whole_line_without_its_line_break_leaves_an_empty_line() {
        assert_eq!(delete("hello\nnext\n", (0, 0), (0, 4)).0, "\nnext\n");
    }

    #[test]
    fn a_range_ending_on_a_line_break_joins_the_next_line() {
        assert_eq!(delete("a\nbb\ncc\n", (1, 0), (1, 2)).0, "a\ncc\n");
        assert_eq!(delete("a\nbb\ncc\n", (0, 1), (1, 0)).0, "ab\ncc\n", "a's break and the first b");
    }

    #[test]
    fn a_range_across_lines_in_either_order() {
        assert_eq!(delete("one\ntwo\nthree\n", (0, 1), (2, 1)), ("oree\n".to_string(), Index2::new(0, 1)));
        assert_eq!(delete("one\ntwo\nthree\n", (2, 1), (0, 1)).0, "oree\n");
    }

    #[test]
    fn the_last_lines_break_isnt_there_to_take() {
        assert_eq!(delete("a\nlast", (1, 0), (1, 4)).0, "a\n");
    }
}
