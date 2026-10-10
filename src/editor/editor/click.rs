//! Where a click lands in the text -- our own, not `edtui`'s: it kept a
//! click right of a line on the line's last character (vim's Normal mode
//! has no cell past it), reported as the caret landing one character left.
//! Mirrors how `edtui` lays the text out: a border, the line-number
//! gutter, lines wrapped character by character at the text width, a tab
//! two cells wide, other characters their Unicode width.
//! History: docs/history/editor-rendering.md.

use edtui::{Index2, Lines, RowIndex};
use unicode_width::UnicodeWidthChar;

use super::Editor;

/// `edtui`'s tab width (its default; we don't set one).
const TAB_WIDTH: usize = 2;

impl Editor {
    /// The text position at screen cell `(column, row)`, as the caret
    /// should land on a click: on the clicked character, after a line's
    /// last one when right of it, at the end of the last line when below
    /// it. `None` before the first draw.
    pub(super) fn text_position_at(&self, column: u16, row: u16) -> Option<Index2> {
        let area = self.view_area;
        let text_width = self.text_width();
        if text_width == 0 || area.height < 3 {
            return None;
        }
        let x = usize::from(column.saturating_sub(self.text_left()));
        let screen_row = usize::from(row.saturating_sub(area.y + 1));
        Some(position_at(&self.state.lines, self.viewport_top_row(), text_width, x, screen_row))
    }
}


/// The position at cell `x` of screen row `screen_row`, with `top` the
/// first buffer row on screen.
fn position_at(lines: &Lines, top: usize, text_width: usize, x: usize, screen_row: usize) -> Index2 {
    let mut rows_left = screen_row;
    let mut row = top;
    loop {
        let Some(line) = lines.get(RowIndex::new(row)) else {
            // Below the text: the end of the last line.
            let last = lines.len().saturating_sub(1);
            return Index2::new(last, lines.get(RowIndex::new(last)).map_or(0, |line| line.len()));
        };
        let segments = wrap(line, text_width);
        if rows_left < segments.len() {
            let segment = segments[rows_left].clone();
            let is_last = rows_left + 1 == segments.len();
            return Index2::new(row, column_in(line, segment, x, is_last));
        }
        rows_left -= segments.len();
        row += 1;
    }
}


/// The column at cell `x` within `segment` of `line`: the character
/// under it, else -- right of the text -- after the line's end on its
/// last segment, on the segment's last character on a wrapped one (the
/// position after it is drawn at the next row's start).
fn column_in(line: &[char], segment: std::ops::Range<usize>, x: usize, is_last: bool) -> usize {
    let mut left = 0;
    for col in segment.clone() {
        left += width(line[col]);
        if x < left {
            return col;
        }
    }
    if is_last || segment.is_empty() {
        segment.end
    } else {
        segment.end - 1
    }
}


/// `line` cut into the character ranges `edtui` draws on one screen row
/// each: a new row when the next character wouldn't fit. An empty line is
/// one empty row.
fn wrap(line: &[char], text_width: usize) -> Vec<std::ops::Range<usize>> {
    let mut segments = Vec::new();
    let (mut start, mut used) = (0, 0);
    for (col, &ch) in line.iter().enumerate() {
        let ch_width = width(ch);
        if used + ch_width > text_width && col > start {
            segments.push(start..col);
            (start, used) = (col, 0);
        }
        used += ch_width;
    }
    segments.push(start..line.len());
    segments
}


fn width(ch: char) -> usize {
    if ch == '\t' {
        TAB_WIDTH
    } else {
        ch.width().unwrap_or(0)
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Lines {
        Lines::from(text)
    }

    #[test]
    fn a_long_line_wraps_by_width_and_an_empty_one_is_one_row() {
        let line: Vec<char> = "abcdefg".chars().collect();
        assert_eq!(wrap(&line, 3), vec![0..3, 3..6, 6..7]);
        assert_eq!(wrap(&[], 3), vec![0..0]);
        let wide: Vec<char> = "a\u{4e2d}b".chars().collect();
        assert_eq!(wrap(&wide, 2), vec![0..1, 1..2, 2..3], "a wide character doesn't fit after \"a\"");
    }

    #[test]
    fn a_click_lands_on_the_character_under_it_and_after_the_end_right_of_it() {
        let text = lines("abc\n\ndef");
        assert_eq!(position_at(&text, 0, 20, 1, 0), Index2::new(0, 1), "on \"b\"");
        assert_eq!(position_at(&text, 0, 20, 9, 0), Index2::new(0, 3), "right of \"abc\"");
        assert_eq!(position_at(&text, 0, 20, 5, 1), Index2::new(1, 0), "an empty line");
        assert_eq!(position_at(&text, 0, 20, 0, 2), Index2::new(2, 0), "\"d\", the line's first character");
        assert_eq!(position_at(&text, 0, 20, 1, 9), Index2::new(2, 3), "below the text: the end");
    }

    #[test]
    fn the_view_may_start_further_down() {
        let text = lines("abc\ndef\nghi");
        assert_eq!(position_at(&text, 1, 20, 2, 1), Index2::new(2, 2));
    }

    #[test]
    fn wrapped_rows_count_as_rows_and_keep_their_columns() {
        // "abcdefg" at width 3: "abc" / "def" / "g".
        let text = lines("abcdefg\nxy");
        assert_eq!(position_at(&text, 0, 3, 1, 1), Index2::new(0, 4), "\"e\"");
        assert_eq!(position_at(&text, 0, 3, 2, 2), Index2::new(0, 7), "right of \"g\": the end");
        assert_eq!(position_at(&text, 0, 3, 0, 3), Index2::new(1, 0), "the next line");
    }

    #[test]
    fn tabs_and_wide_characters_take_their_cells() {
        let text = lines("\tx\n\u{4e2d}y");
        assert_eq!(position_at(&text, 0, 20, 1, 0), Index2::new(0, 0), "a tab's second cell");
        assert_eq!(position_at(&text, 0, 20, 2, 0), Index2::new(0, 1), "\"x\" after it");
        assert_eq!(position_at(&text, 0, 20, 1, 1), Index2::new(1, 0), "a wide character's second cell");
        assert_eq!(position_at(&text, 0, 20, 2, 1), Index2::new(1, 1), "\"y\"");
    }
}
