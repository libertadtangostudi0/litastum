use edtui::{Highlight, Index2, Lines, RowIndex};
use ratatui::style::Style;

/// VS Code-style "highlight every other occurrence of the identifier
/// under the cursor" -- requested directly, with a screenshot of VS
/// Code's own behavior as the reference. No selection needed, purely a
/// read-only visual aid: this only ever *reads* `lines`/`cursor`, never
/// mutates the buffer.
///
/// Built on `edtui`'s own `EditorState::highlights` field
/// (`state::highlight::Highlight`) rather than a hand-rolled render
/// pass -- confirmed directly from `edtui`'s source
/// (`view/internal.rs::line_into_spans_with_selections`/
/// `line_into_highlighted_spans_with_selections`) that this field is
/// already rendered every frame, layered between the base/syntax
/// styling and an active selection ("selection takes priority, then
/// highlights, then base") -- exactly the layering this needs, with no
/// `EditorView` changes required at all. `TODO/editor.md` had speculated this
/// would need a second hand-rolled render pass; it doesn't.
///
/// `Highlight`'s own style *replaces* whatever span it lands on outright
/// (`InternalSpan::split_spans`, confirmed by reading it directly) --
/// there's no way to tint just the background while leaving a token's
/// own syntax color underneath. `style` is expected to set both `fg`
/// and `bg` for exactly this reason, the same tradeoff this codebase's
/// own text-selection highlighting already accepts (selected text also
/// renders in one flat color, not per-token syntax colors).
///
/// No `CharacterClass` reimplementation needed here (unlike
/// `bindings::word_select`'s own history of avoiding exactly that,
/// since `edtui`'s is `pub(crate)`) -- a word character is defined
/// locally (`is_word_char`), matching `edtui`'s own internal
/// `CharacterClass::Alphanumeric` definition (ASCII alphanumeric or
/// underscore) since there's no need to reach into the crate for
/// something this simple.
pub(super) fn word_occurrence_highlights(lines: &Lines, cursor: Index2, style: Style) -> Vec<Highlight> {
    let Some((home_start, _home_end, word)) = word_at(lines, cursor) else {
        return Vec::new();
    };

    word_occurrences(lines, &word)
        .into_iter()
        .filter(|&(row, start, _end)| !(row == cursor.row && start == home_start))
        .map(|(row, start, end)| Highlight::new(Index2::new(row, start), Index2::new(row, end.saturating_sub(1)), style))
        .collect()
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// The identifier `cursor` currently sits on, if any -- `(start_col,
/// end_col_exclusive, word)` on `cursor`'s own row. `None` when there's
/// no word character either under the cursor or immediately to its left
/// (whitespace, punctuation, or an empty line), matching VS Code's own
/// behavior of showing no highlights at all in that case rather than
/// guessing at a nearby word.
///
/// Checks the cursor's own cell *first*, falling back to the cell
/// immediately to its left only if that one isn't a word character --
/// reported directly as a real bug: placing the cursor right after a
/// word (its own "touching from the right" wording), e.g. the
/// insert-mode append position past a line's last character, or the
/// boundary column between a word and the punctuation right after it
/// (`"theme.rs"`'s cursor sitting right after `"theme"`, before the
/// `.`), highlighted nothing at all. `row[cursor.col]` alone only ever
/// covers "the cursor sits ON a word character" -- it can't also cover
/// "the cursor sits one column past a word's own last character,"
/// which is exactly where editing normally leaves the cursor after
/// typing or moving past a word.
///
/// Both branches re-check `< row.len()` before indexing, not just
/// `> 0` -- a second real crash, reported directly: `MoveUp`/`MoveDown`
/// (confirmed from `edtui`'s own source, see `.claude/rules/litastum-stack.md`'s
/// word-selection history) only ever change `cursor.row`, never
/// `.col` -- so moving from a long line onto a short or empty one
/// leaves `cursor.col` sitting well past that new line's own length
/// until a horizontal move re-clamps it. `row[cursor.col - 1]` on an
/// empty row (`row.len() == 0`) with `cursor.col` still 35 from the
/// previous line panicked with an out-of-bounds index -- `cursor.col >
/// 0` alone doesn't guarantee `cursor.col - 1` is actually a valid
/// index into *this* row.
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

/// Every whole-word occurrence of `word` across the buffer, as
/// `(row, start_col, end_col_exclusive)` -- a match only counts if it
/// isn't itself flanked by another word character (so searching for
/// `"log"` doesn't also light up the `"log"` inside `"logger"`), the
/// same "identifier, not substring" rule VS Code's own highlight uses.
/// A plain, un-indexed scan -- fine for the file sizes this editor
/// targets (see `find_file/search.rs`'s own similar scope note), not
/// built for huge files.
fn word_occurrences(lines: &Lines, word: &str) -> Vec<(usize, usize, usize)> {
    let word_len = word.chars().count();
    if word_len == 0 {
        return Vec::new();
    }

    let word_chars: Vec<char> = word.chars().collect();
    let mut occurrences = Vec::new();
    for row_index in 0..lines.len() {
        let Some(row) = lines.get(RowIndex::new(row_index)) else {
            continue;
        };
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

    #[test]
    fn highlights_every_other_occurrence_not_the_one_under_the_cursor() {
        let lines = Lines::from("command command_line\ncommand");
        // Cursor on the first "command" (row 0, col 0).
        let highlights = word_occurrence_highlights(&lines, Index2::new(0, 0), style());

        // Not "command_line" (substring, not a whole word) and not the
        // occurrence the cursor itself sits on -- only row 1's "command".
        assert_eq!(highlights, vec![Highlight::new(Index2::new(1, 0), Index2::new(1, 6), style())]);
    }

    #[test]
    fn no_highlights_when_the_cursor_is_not_on_or_next_to_a_word_character() {
        let lines = Lines::from("foo  bar"); // two spaces -- neither is adjacent to a word
        let highlights = word_occurrence_highlights(&lines, Index2::new(0, 4), style());

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
        let highlights = word_occurrence_highlights(&lines, Index2::new(1, 34), style());

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
        let highlights = word_occurrence_highlights(&lines, Index2::new(0, 5), style());

        assert_eq!(highlights, vec![Highlight::new(Index2::new(1, 0), Index2::new(1, 4), style())]);
    }

    /// Same edge case at the very end of a line -- the insert-mode
    /// "append" cursor position past the last real character.
    #[test]
    fn touching_a_word_at_the_end_of_a_line_still_highlights_other_occurrences() {
        let lines = Lines::from("theme\ntheme");
        let highlights = word_occurrence_highlights(&lines, Index2::new(0, 5), style()); // past the 'e'

        assert_eq!(highlights, vec![Highlight::new(Index2::new(1, 0), Index2::new(1, 4), style())]);
    }

    #[test]
    fn no_highlights_when_the_word_appears_only_once() {
        let lines = Lines::from("unique");
        let highlights = word_occurrence_highlights(&lines, Index2::new(0, 0), style());

        assert!(highlights.is_empty());
    }

    #[test]
    fn does_not_match_a_word_that_is_only_a_substring() {
        let lines = Lines::from("log logger catalog");
        let highlights = word_occurrence_highlights(&lines, Index2::new(0, 0), style());

        assert!(highlights.is_empty(), "\"logger\"/\"catalog\" contain \"log\" but aren't the whole word \"log\"");
    }

    #[test]
    fn matches_underscored_identifiers_as_one_word() {
        let lines = Lines::from("my_var = 1\nprint(my_var)");
        let highlights = word_occurrence_highlights(&lines, Index2::new(0, 0), style());

        assert_eq!(highlights, vec![Highlight::new(Index2::new(1, 6), Index2::new(1, 11), style())]);
    }
}
