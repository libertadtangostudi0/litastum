use std::ops::Range;

use edtui::{Highlight, Index2, Lines, RowIndex};
use ratatui::style::Style;

/// VS Code-style highlights of every other occurrence of the identifier
/// under the cursor, as `edtui` `Highlight`s (rendered by `edtui` between
/// syntax styling and the selection). A highlight replaces the span's
/// style outright, so `style` sets both `fg` and `bg`. Word characters
/// match `edtui`'s own `CharacterClass::Alphanumeric`.
///
/// Scans only `rows` -- the whole-buffer scan cost ~30ms per frame on
/// 100k lines. `Editor::view` passes a window that covers wherever
/// `edtui` can scroll to. History: docs/history/editor-rendering.md.
pub(super) fn word_occurrence_highlights(lines: &Lines, cursor: Index2, rows: Range<usize>, style: Style) -> Vec<Highlight> {
    let Some((home_start, _home_end, word)) = word_at(lines, cursor) else {
        return Vec::new();
    };

    word_occurrences(lines, &word, rows)
        .into_iter()
        .filter(|&(row, start, _end)| !(row == cursor.row && start == home_start))
        .map(|(row, start, end)| Highlight::new(Index2::new(row, start), Index2::new(row, end.saturating_sub(1)), style))
        .collect()
}

/// Longest line the per-frame, un-indexed scans (this module and syntax
/// highlighting) handle; a longer one made every redraw slow. Twice VS
/// Code's ~10,000-character tokenization cap. History: docs/history/editor-rendering.md.
pub(super) const MAX_HIGHLIGHTED_LINE_LEN: usize = 20_000;

/// Whether any line exceeds `MAX_HIGHLIGHTED_LINE_LEN`.
pub(super) fn has_pathologically_long_line(lines: &Lines) -> bool {
    (0..lines.len()).any(|row_index| lines.get(RowIndex::new(row_index)).is_some_and(|row| row.len() > MAX_HIGHLIGHTED_LINE_LEN))
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// The identifier at `cursor` as `(start_col, end_col_exclusive, word)`:
/// the cell under the cursor, else the one to its left (the cursor just
/// past a word, as after typing it). `None` otherwise, like VS Code.
///
/// Both checks bound by `row.len()`: `MoveUp`/`MoveDown` leave
/// `cursor.col` past a shorter line's end, which once panicked here.
fn word_at(lines: &Lines, cursor: Index2) -> Option<(usize, usize, String)> {
    let row = lines.get(RowIndex::new(cursor.row))?;

    let anchor_col = if cursor.col < row.len() && is_word_char(row[cursor.col]) {
        cursor.col
    } else if cursor.col > 0 && cursor.col - 1 < row.len() && is_word_char(row[cursor.col - 1]) {
        cursor.col - 1
    } else {
        return None;
    };

    let mut start = anchor_col;
    while start > 0 && is_word_char(row[start - 1]) {
        start -= 1;
    }
    let mut end = anchor_col + 1;
    while end < row.len() && is_word_char(row[end]) {
        end += 1;
    }

    Some((start, end, row[start..end].iter().collect()))
}

/// Whole-word occurrences of `word` in `rows`, as `(row, start_col,
/// end_col_exclusive)` -- not flanked by another word character, so
/// `log` doesn't match inside `logger`. Skips over-long lines.
fn word_occurrences(lines: &Lines, word: &str, rows: Range<usize>) -> Vec<(usize, usize, usize)> {
    let word_len = word.chars().count();
    if word_len == 0 {
        return Vec::new();
    }

    let word_chars: Vec<char> = word.chars().collect();
    let mut occurrences = Vec::new();
    for row_index in rows.start..rows.end.min(lines.len()) {
        let Some(row) = lines.get(RowIndex::new(row_index)) else {
            continue;
        };
        if row.len() > MAX_HIGHLIGHTED_LINE_LEN {
            continue;
        }
        let mut col = 0;
        while col + word_len <= row.len() {
            let candidate_is_match = row[col..col + word_len] == word_chars[..];
            let left_boundary = col == 0 || !is_word_char(row[col - 1]);
            let right_boundary = col + word_len == row.len() || !is_word_char(row[col + word_len]);

            if candidate_is_match && left_boundary && right_boundary {
                occurrences.push((row_index, col, col + word_len));
                col += word_len;
            } else {
                col += 1;
            }
        }
    }
    occurrences
}


#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    fn style() -> Style {
        Style::default().fg(Color::White).bg(Color::Blue)
    }

    fn word_occurrence_highlights_all(lines: &Lines, cursor: Index2, style: Style) -> Vec<Highlight> {
        word_occurrence_highlights(lines, cursor, 0..usize::MAX, style)
    }

    /// Only rows inside the given window are scanned -- the per-frame
    /// cost fix, see `word_occurrence_highlights`'s own doc comment.
    #[test]
    fn occurrences_outside_the_row_window_are_not_reported() {
        let lines = Lines::from("word
word
word
word");
        let highlights = word_occurrence_highlights(&lines, Index2::new(0, 0), 0..2, style());

        assert_eq!(highlights, vec![Highlight::new(Index2::new(1, 0), Index2::new(1, 3), style())]);
    }

    #[test]
    fn highlights_every_other_occurrence_not_the_one_under_the_cursor() {
        let lines = Lines::from("command command_line\ncommand");
        // Cursor on the first "command" (row 0, col 0).
        let highlights = word_occurrence_highlights_all(&lines, Index2::new(0, 0), style());

        // Not "command_line" (substring, not a whole word) and not the
        // occurrence the cursor itself sits on -- only row 1's "command".
        assert_eq!(highlights, vec![Highlight::new(Index2::new(1, 0), Index2::new(1, 6), style())]);
    }

    #[test]
    fn no_highlights_when_the_cursor_is_not_on_or_next_to_a_word_character() {
        let lines = Lines::from("foo  bar"); // two spaces -- neither is adjacent to a word
        let highlights = word_occurrence_highlights_all(&lines, Index2::new(0, 4), style());

        assert!(highlights.is_empty());
    }

    /// Regression test for the real crash: `MoveUp`/`MoveDown` only
    /// ever change `cursor.row`, never `.col` -- moving from a long
    /// line onto a shorter (or, here, completely empty) one leaves
    /// `cursor.col` sitting well past the new row's own length.
    /// `row[cursor.col - 1]` on an empty row with a stale, far-too-large
    /// `cursor.col` panicked with an out-of-bounds index; must return
    /// `None` instead.
    #[test]
    fn does_not_panic_when_the_cursor_column_is_stale_on_an_empty_row() {
        let lines = Lines::from("a very long line up above\n");
        let highlights = word_occurrence_highlights_all(&lines, Index2::new(1, 34), style());

        assert!(highlights.is_empty());
    }

    /// Regression test for the real report: placing the cursor right
    /// after a word (not on one of its own characters, the insert-mode
    /// "append" position or the boundary column right before the next
    /// punctuation) highlighted nothing at all -- reported directly
    /// against `"theme.rs"`, cursor sitting right after `"theme"`,
    /// before the `.`.
    #[test]
    fn touching_a_word_from_its_own_right_edge_still_highlights_other_occurrences() {
        let lines = Lines::from("theme.rs\ntheme.rs");
        // Cursor right after "theme" (col 5), not on any of its own
        // characters -- row[5] is '.'.
        let highlights = word_occurrence_highlights_all(&lines, Index2::new(0, 5), style());

        assert_eq!(highlights, vec![Highlight::new(Index2::new(1, 0), Index2::new(1, 4), style())]);
    }

    /// Same edge case at the very end of a line -- the insert-mode
    /// "append" cursor position past the last real character.
    #[test]
    fn touching_a_word_at_the_end_of_a_line_still_highlights_other_occurrences() {
        let lines = Lines::from("theme\ntheme");
        let highlights = word_occurrence_highlights_all(&lines, Index2::new(0, 5), style()); // past the 'e'

        assert_eq!(highlights, vec![Highlight::new(Index2::new(1, 0), Index2::new(1, 4), style())]);
    }

    #[test]
    fn no_highlights_when_the_word_appears_only_once() {
        let lines = Lines::from("unique");
        let highlights = word_occurrence_highlights_all(&lines, Index2::new(0, 0), style());

        assert!(highlights.is_empty());
    }

    #[test]
    fn does_not_match_a_word_that_is_only_a_substring() {
        let lines = Lines::from("log logger catalog");
        let highlights = word_occurrence_highlights_all(&lines, Index2::new(0, 0), style());

        assert!(highlights.is_empty(), "\"logger\"/\"catalog\" contain \"log\" but aren't the whole word \"log\"");
    }

    #[test]
    fn matches_underscored_identifiers_as_one_word() {
        let lines = Lines::from("my_var = 1\nprint(my_var)");
        let highlights = word_occurrence_highlights_all(&lines, Index2::new(0, 0), style());

        assert_eq!(highlights, vec![Highlight::new(Index2::new(1, 6), Index2::new(1, 11), style())]);
    }

    /// A match on an over-long row is never reported, while one on a normal
    /// row still is. History: docs/history/editor-rendering.md.
    #[test]
    fn a_pathologically_long_line_is_skipped_by_the_word_scan() {
        let long_row = format!("needle {}", "x".repeat(MAX_HIGHLIGHTED_LINE_LEN + 1));
        let lines = Lines::from(format!("{long_row}\nneedle"));

        let highlights = word_occurrence_highlights_all(&lines, Index2::new(1, 0), style());

        assert_eq!(highlights, Vec::new(), "the long row's own match must not be reported");
    }

    #[test]
    fn has_pathologically_long_line_detects_and_ignores_short_files() {
        let short = Lines::from("fn main() {}\nlet x = 1;");
        assert!(!has_pathologically_long_line(&short));

        let long_row = "x".repeat(MAX_HIGHLIGHTED_LINE_LEN + 1);
        let long = Lines::from(long_row);
        assert!(has_pathologically_long_line(&long));
    }
}
