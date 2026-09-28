use edtui::{Highlight, Index2, Lines, RowIndex};
use ratatui::style::Style;

/// The bracket kinds this editor understands, each as `(open, close)`.
/// `()`/`[]`/`{}` are unambiguous everywhere they appear. `<>` is
/// included too, requested directly -- but it's a different case from
/// the other three: `<`/`>` are also comparison operators in every
/// C-like language this editor highlights, so a same-kind nesting-depth
/// scan (this module's whole strategy, see `find_forward`/`find_backward`)
/// can genuinely mismatch on real code containing one (e.g. `a < b`
/// with a real, unrelated `>` later in the file) -- there's no syntax
/// awareness here to tell "comparison" from "generic/tag delimiter"
/// apart, unlike a real parser-backed matcher. Included anyway because
/// this is the same tradeoff most mainstream editors that support `<>`
/// matching at all already accept (a plain depth scan, not a parser),
/// and it's still correct far more often than not -- genuinely
/// unbalanced `<`/`>` as bare comparisons on their own line, or spread
/// across a whole file, is a much rarer shape than balanced angle
/// brackets in generics (`Vec<Option<T>>`) or HTML/XML tags
/// (`<div>...</div>`), which is exactly where this earns its keep. No
/// quote-pair or other custom-delimiter matching attempted.
const PAIRS: [(char, char); 4] = [('(', ')'), ('[', ']'), ('{', '}'), ('<', '>')];

/// Highlights *both* the bracket under (or immediately left of) the
/// cursor and its matching partner, VS Code/Far Manager-style -- two
/// single-cell `Highlight`s, or none at all if the cursor isn't
/// touching a bracket or the bracket has no match (unbalanced code, or
/// the match is outside the buffer entirely).
///
/// Both brackets are included, not just the far one -- requested
/// directly, and matches real Far/VS Code behavior (both sides of a
/// matched pair read as "this pair," not just one of them). `edtui`
/// paints the cursor's own cell *after* every other style
/// (`EditorView::render`, same "cursor on top" behavior
/// docs/history/word-select.md (5) records for text selection), which
/// would otherwise silently hide the near bracket's own highlight for
/// as long as the cursor sits right on it -- `Editor::view`'s
/// `cursor_style` decision compensates for that separately (see
/// `cursor_is_on_a_matched_bracket`'s own doc comment), painting the
/// cursor's cell with this same style directly rather than relying on
/// the `Highlight` this function returns for that cell.
///
/// Same highlight color as `word_highlight.rs`'s "same word as the one
/// under the cursor" feature -- requested directly too, so brackets
/// read as the same *kind* of "matches something nearby" hint rather
/// than a visually distinct feature (`Editor::view` passes both passes
/// the identical `Style`).
///
/// Deliberately a *separate* highlight pass from `word_highlight.rs`,
/// not folded into it — so bracket matching doesn't get swept into
/// word-occurrence highlighting's own notion of "similar," and in
/// particular keeps its own "highlight both, not just the home
/// occurrence" behavior independent of `word_highlight`'s opposite
/// choice (excluding the cursor's own word occurrence). The two
/// features can never actually collide in practice (`word_highlight`'s
/// own `is_word_char` only ever matches ASCII alphanumerics and `_`, so
/// a bracket character is never a candidate for it to begin with), but
/// keeping this in its own module/function with its own call in
/// `Editor::view` — rather than, say, extending `word_occurrence_highlights`
/// to also special-case brackets — keeps that guarantee structural
/// instead of incidental.
///
/// `edtui`'s own `Highlight` fully *replaces* whatever style was
/// already on that cell (no fg/bg merging — confirmed directly from its
/// source, same tradeoff `word_highlight.rs` already documents), so
/// `style` is expected to set both `fg` and `bg` for that reason.
pub(super) fn bracket_match_highlights(lines: &Lines, cursor: Index2, style: Style) -> Vec<Highlight> {
    let Some((pos, ch)) = bracket_at(lines, cursor) else {
        return Vec::new();
    };
    let Some(match_pos) = find_matching_bracket(lines, pos, ch) else {
        return Vec::new();
    };
    vec![Highlight::new(pos, pos, style), Highlight::new(match_pos, match_pos, style)]
}

/// Whether `cursor` sits *exactly* on one side of a genuinely matched
/// bracket pair -- not merely "touching" one from the append position
/// one column to its right (`bracket_at`'s own doc comment), since in
/// that case the cursor's own screen cell isn't actually a bracket
/// character at all. Used by `Editor::view` to decide whether to paint
/// the real terminal cursor's own cell in the highlight color instead
/// of the plain base style: `edtui` paints that cell *after* any
/// `Highlight` (`EditorView::render`), which would otherwise silently
/// hide the near bracket's own highlight for as long as the cursor sits
/// right on it -- reported directly, with a screenshot, once
/// `bracket_match_highlights` started returning both brackets: the far
/// one visibly highlighted, the near one (under the cursor) not, even
/// though both are meant to show at once. Same fix shape already
/// applied to an active text selection's own cursor cell
/// (`Editor::view`'s own doc comment on `cursor_style`), just for
/// bracket matching instead of selection.
pub(super) fn cursor_is_on_a_matched_bracket(lines: &Lines, cursor: Index2) -> bool {
    let Some((pos, ch)) = bracket_at(lines, cursor) else {
        return false;
    };
    pos == cursor && find_matching_bracket(lines, pos, ch).is_some()
}

/// The inclusive `(top_row, bottom_row)` span of a matched bracket pair
/// touching `cursor` -- `None` if the cursor isn't touching a bracket,
/// the bracket has no match, or both sides sit on the same row (nothing
/// to scroll for). Used by `Editor::view` to widen the viewport when
/// the pair's own two rows would otherwise not both fit on screen --
/// reported directly, with a screenshot: a multi-line pair only ever
/// showed one bracket highlighted whenever the other one scrolled
/// outside the visible area, since `edtui`'s own auto-scroll only ever
/// keeps the *cursor's* row in view, with no notion of "and also this
/// other row." Not a highlighting bug at all -- there's no cell to
/// paint a color on if it was never rendered to the terminal.
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
