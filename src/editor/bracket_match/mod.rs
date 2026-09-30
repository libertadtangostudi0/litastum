use edtui::{Highlight, Index2, Lines, RowIndex};
use ratatui::style::Style;

/// Bracket kinds as `(open, close)`. `<>` can mismatch where `<`/`>`
/// are comparisons (a depth scan has no syntax awareness) -- accepted,
/// like most editors, for generics and tags. No quote matching.
const PAIRS: [(char, char); 4] = [('(', ')'), ('[', ']'), ('{', '}'), ('<', '>')];

/// Highlights both the bracket under (or just left of) the cursor and
/// its partner, in the word-occurrence style; none if there's no bracket
/// or no match. `edtui` paints the cursor cell last, so `Editor::view`
/// repaints it separately (`cursor_is_on_a_matched_bracket`). A separate
/// pass from `word_highlight`, so their rules stay independent. `style`
/// sets both `fg` and `bg`: a `Highlight` replaces the span's style.
/// History: docs/history/editor-rendering.md.
pub(super) fn bracket_match_highlights(lines: &Lines, cursor: Index2, style: Style) -> Vec<Highlight> {
    let Some((pos, ch)) = bracket_at(lines, cursor) else {
        return Vec::new();
    };
    let Some(match_pos) = find_matching_bracket(lines, pos, ch) else {
        return Vec::new();
    };
    vec![Highlight::new(pos, pos, style), Highlight::new(match_pos, match_pos, style)]
}

/// Whether `cursor` sits exactly on a matched bracket (not just touching
/// it from the right). `Editor::view` then paints the cursor cell in the
/// highlight color, since `edtui` paints it after every `Highlight`.
pub(super) fn cursor_is_on_a_matched_bracket(lines: &Lines, cursor: Index2) -> bool {
    let Some((pos, ch)) = bracket_at(lines, cursor) else {
        return false;
    };
    pos == cursor && find_matching_bracket(lines, pos, ch).is_some()
}

/// Rows `(top, bottom)` of a matched pair touching `cursor`; `None` if
/// there's no pair or it's on one row. `Editor::view` widens the viewport
/// to fit both -- `edtui` only keeps the cursor's row in view, so the
/// other bracket was never drawn. History: docs/history/editor-rendering.md.
pub(super) fn matched_bracket_row_span(lines: &Lines, cursor: Index2) -> Option<(usize, usize)> {
    let (pos, ch) = bracket_at(lines, cursor)?;
    let match_pos = find_matching_bracket(lines, pos, ch)?;
    if pos.row == match_pos.row {
        return None;
    }
    Some((pos.row.min(match_pos.row), pos.row.max(match_pos.row)))
}

fn is_bracket(c: char) -> bool {
    PAIRS.iter().any(|&(open, close)| c == open || c == close)
}

/// The bracket character at the cursor's own cell, or (matching
/// `word_highlight::word_at`'s own "touching" convention, for the same
/// reason -- the cursor's insert-mode "append" position sits one column
/// past the last real character) the cell immediately to its left if
/// that one is a bracket instead. `None` if neither is.
fn bracket_at(lines: &Lines, cursor: Index2) -> Option<(Index2, char)> {
    let row = lines.get(RowIndex::new(cursor.row))?;

    if cursor.col < row.len() && is_bracket(row[cursor.col]) {
        return Some((cursor, row[cursor.col]));
    }
    if cursor.col > 0 && cursor.col - 1 < row.len() && is_bracket(row[cursor.col - 1]) {
        let pos = Index2::new(cursor.row, cursor.col - 1);
        return Some((pos, row[pos.col]));
    }
    None
}

/// Finds `from`'s matching bracket -- scans forward (tracking nesting
/// depth of this same bracket *kind* only; a `{` inside `(...)` is
/// invisible to matching a `(`, the same "same kind only" rule every
/// mainstream bracket-matcher uses) if `from` holds an opening bracket,
/// or backward if it holds a closing one. `None` for unbalanced code or
/// a match that would fall outside the buffer.
fn find_matching_bracket(lines: &Lines, from: Index2, ch: char) -> Option<Index2> {
    let &(open, close) = PAIRS.iter().find(|&&(open, close)| ch == open || ch == close)?;
    if ch == open {
        find_forward(lines, from, open, close)
    } else {
        find_backward(lines, from, open, close)
    }
}

fn find_forward(lines: &Lines, from: Index2, open: char, close: char) -> Option<Index2> {
    let mut depth = 0u32;
    for row_index in from.row..lines.len() {
        let row = lines.get(RowIndex::new(row_index))?;
        let start_col = if row_index == from.row { from.col + 1 } else { 0 };
        for col in start_col..row.len() {
            let c = row[col];
            if c == open {
                depth += 1;
            } else if c == close {
                if depth == 0 {
                    return Some(Index2::new(row_index, col));
                }
                depth -= 1;
            }
        }
    }
    None
}

fn find_backward(lines: &Lines, from: Index2, open: char, close: char) -> Option<Index2> {
    let mut depth = 0u32;
    for row_index in (0..=from.row).rev() {
        let row = lines.get(RowIndex::new(row_index))?;
        let end_col = if row_index == from.row { from.col } else { row.len() };
        for col in (0..end_col).rev() {
            let c = row[col];
            if c == close {
                depth += 1;
            } else if c == open {
                if depth == 0 {
                    return Some(Index2::new(row_index, col));
                }
                depth -= 1;
            }
        }
    }
    None
}


#[cfg(test)]
mod tests;
