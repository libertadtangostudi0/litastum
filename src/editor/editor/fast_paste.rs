use edtui::{EditorMode, Index2, Lines, RowIndex};
use tracing::warn;

use super::Editor;

impl Editor {
    /// `Ctrl+V`: pastes the real OS clipboard's text at the cursor,
    /// bypassing `edtui`'s own `PasteBefore` action entirely.
    ///
    /// Reported directly as unusably slow for a long pasted line --
    /// confirmed straight from source (`edtui` 0.11.7's
    /// `actions/cpaste.rs::paste` -> `helper::insert_str` ->
    /// `insert_char`, once *per character*) that this is a real O(n²)
    /// cost: each of the N pasted characters does its own single-element
    /// `Vec::insert` at a column that keeps growing, an O(current line
    /// length) shift, every single time. `edtui-jagged`'s own `Jagged`
    /// type (`state.lines`'s real type) has no bulk "splice N elements
    /// into a row" API to have used instead -- only a single-element
    /// insert (`JaggedSlice<T> for T`) or a whole-new-row insert
    /// (`JaggedSlice<T> for Vec<T>`, keyed by `RowIndex`, not a
    /// column). This isn't something to patch in `edtui`/`edtui-jagged`
    /// themselves -- handing an AI-assisted fix to a third-party crate
    /// without real review isn't something to do lightly -- so this
    /// stays local: `state.lines` is a public field and
    /// `Jagged::get_mut`/`insert` are both public, so `splice_paste`
    /// below can splice the row's own backing `Vec<char>` directly
    /// (`std::vec::Vec::splice`, one O(line length) pass total for the
    /// common single-line-paste case) without needing anything
    /// crate-private from `edtui` at all. See `input`'s own doc comment
    /// for how this stays undoable as one atomic block
    /// (`push_undo_snapshot`) regardless of `edtui`'s own inaccessible
    /// `capture()`.
    pub(super) fn fast_paste_from_clipboard(&mut self) {
        let text = match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_text()) {
            Ok(text) => text,
            Err(err) => {
                warn!(%err, "editor: clipboard unavailable for paste");
                return;
            }
        };
        self.paste_text(&text);
    }

    /// The actual splice + undo-snapshot + selection bookkeeping
    /// `fast_paste_from_clipboard` (`Ctrl+V`) uses, factored out so
    /// `event_loop::paste` (a real terminal bracketed paste, `Event::Paste`, and
    /// the Windows `GetAsyncKeyState` `Ctrl+V` bypass) can feed it text
    /// directly, without a redundant clipboard read of its own. A no-op
    /// for empty `text` -- also what makes this safe to call for every
    /// keystroke `event_loop` might ever route here, not just a real paste.
    pub(crate) fn paste_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        self.push_undo_snapshot();
        self.state.cursor = splice_paste(&mut self.state.lines, self.state.cursor, text);

        // Matches the pre-existing simplification for `Ctrl+V` over an
        // active selection (see `bindings.rs`'s own module doc comment):
        // clears the selection and lands in typing mode rather than
        // replacing the selected text -- unchanged behavior, just
        // reached without going through `edtui`'s own dispatch this
        // time, so it has to be replicated here by hand.
        if self.state.mode == EditorMode::Visual {
            self.state.selection = None;
            self.state.mode = EditorMode::Insert;
        }

        self.buffer_changed();
    }
}


/// Splices `text` into `lines` starting at `cursor`, in
/// O(pasted length + line length) rather than `edtui`'s own `PasteBefore`
/// (O(pasted length * line length) -- see
/// `Editor::fast_paste_from_clipboard`'s own doc comment for the full
/// story on why this exists as a from-scratch implementation instead of
/// calling into `edtui` at all).
///
/// A no-op (returns `cursor` unchanged) for empty `text` -- callers
/// should already be checking this themselves before saving an undo
/// snapshot, but this stays defensive since it's the one thing that
/// actually decides whether anything happens to the buffer.
///
/// Returns the cursor's new position: for single-line `text` (no `\n`,
/// by far the common case -- typing/pasting a URL, a JSON blob, a log
/// line, ...), that's the last character actually pasted, matching
/// `edtui`'s own `PasteBefore` (vim `P`) convention. For multi-line
/// `text`, that's the end of the pasted content on its own new last
/// row -- a new convention, not `edtui`'s own vim-specific linewise/
/// characterwise `P` rules, which a plain `Ctrl+V` in this app's
/// non-modal keymap was never trying to replicate in the first place.
fn splice_paste(lines: &mut Lines, cursor: Index2, text: &str) -> Index2 {
    // `\r\n`/bare `\r` line endings collapse to `\n` here, matching
    // `edtui`'s own single-line-mode paste handling (`actions/cpaste.rs`)
    // and this project's own `text_field`/command-line paste handling
    // (`explorer::confirm::paste_from_clipboard`) -- a `\r` left in
    // would otherwise become a literal, visible character in the buffer
    // rather than part of the line break.
    let normalized = text.replace('\r', "");
    if normalized.is_empty() {
        return cursor;
    }

    // `Lines::from("")` (a brand-new, still-empty buffer) has *zero*
    // rows, not one empty row -- `lines.get_mut(RowIndex::new(0))` would
    // otherwise return `None` and silently drop the whole paste.
    if lines.is_empty() {
        lines.push(Vec::<char>::new());
    }

    let row = cursor.row;
    let max_col = lines.len_col(row).unwrap_or(0);
    let col = cursor.col.min(max_col);
    let segments: Vec<&str> = normalized.split('\n').collect();

    if segments.len() == 1 {
        return splice_single_line(lines, row, col, segments[0]);
    }
    splice_multi_line(lines, row, col, &segments)
}

/// The common case: no newline in the pasted text at all, so this only
/// ever touches the one row the cursor is already on -- a single
/// `Vec::splice` call, the actual fix for the reported O(n²) blowup.
fn splice_single_line(lines: &mut Lines, row: usize, col: usize, segment: &str) -> Index2 {
    let chars: Vec<char> = segment.chars().collect();
    let Some(row_vec) = lines.get_mut(RowIndex::new(row)) else {
        return Index2::new(row, col);
    };
    let pasted_len = chars.len();
    row_vec.splice(col..col, chars);
    Index2::new(row, col + pasted_len.saturating_sub(1))
}

/// The pasted text contains at least one newline: the row the cursor
/// was on splits into "head + first pasted segment" (kept as the same
/// row) and "last pasted segment + original tail" (a new row), with any
/// segments strictly in between inserted as whole new rows of their
/// own. Every one of these is either a plain `Vec` extend/splice on one
/// row or a whole-row insert (`Jagged::insert`, itself just an outer
/// `Vec<Vec<char>>::insert` -- one shift proportional to the *row
/// count*, not to any row's own length) -- no per-character cost
/// against a long row anywhere in this path either.
fn splice_multi_line(lines: &mut Lines, row: usize, col: usize, segments: &[&str]) -> Index2 {
    let Some(row_vec) = lines.get_mut(RowIndex::new(row)) else {
        return Index2::new(row, col);
    };
    let tail: Vec<char> = row_vec.split_off(col);
    row_vec.extend(segments[0].chars());

    let mut insert_at = row + 1;
    for segment in &segments[1..segments.len() - 1] {
        lines.insert(RowIndex::new(insert_at), segment.chars().collect::<Vec<char>>());
        insert_at += 1;
    }

    let mut last_row: Vec<char> = segments[segments.len() - 1].chars().collect();
    let last_len = last_row.len();
    last_row.extend(tail);
    lines.insert(RowIndex::new(insert_at), last_row);

    Index2::new(insert_at, last_len.saturating_sub(1))
}


#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(lines: &Lines) -> String {
        lines.clone().to_string()
    }

    #[test]
    fn empty_text_is_a_no_op() {
        let mut lines = Lines::from("hello");
        let cursor = Index2::new(0, 2);

        let result = splice_paste(&mut lines, cursor, "");

        assert_eq!(text_of(&lines), "hello");
        assert_eq!(result, cursor);
    }

    #[test]
    fn single_line_paste_splices_into_the_current_row() {
        let mut lines = Lines::from("held");
        // Cursor between "he" and "ld" -- pasting "llo wor" should give
        // "hello world".
        let cursor = Index2::new(0, 2);

        let result = splice_paste(&mut lines, cursor, "llo wor");

        assert_eq!(text_of(&lines), "hello world");
        // Lands on the last pasted character ('r'), matching PasteBefore's
        // own vim-`P` convention.
        assert_eq!(result, Index2::new(0, 8));
    }

    #[test]
    fn single_line_paste_at_the_very_end_of_the_row() {
        let mut lines = Lines::from("hello");
        let cursor = Index2::new(0, 5);

        let result = splice_paste(&mut lines, cursor, "!");

        assert_eq!(text_of(&lines), "hello!");
        assert_eq!(result, Index2::new(0, 5));
    }

    #[test]
    fn cursor_column_past_the_line_end_clamps_instead_of_panicking() {
        let mut lines = Lines::from("hi");
        let cursor = Index2::new(0, 99);

        let result = splice_paste(&mut lines, cursor, "!");

        assert_eq!(text_of(&lines), "hi!");
        assert_eq!(result, Index2::new(0, 2));
    }

    #[test]
    fn carriage_returns_are_stripped_not_pasted_as_literal_characters() {
        let mut lines = Lines::from("ab");
        let cursor = Index2::new(0, 1);

        splice_paste(&mut lines, cursor, "X\r\nY");

        assert_eq!(text_of(&lines), "aX\nYb", "\\r\\n should collapse to a plain line break, no stray \\r left in the buffer");
    }

    #[test]
    fn multi_line_paste_splits_the_row_and_inserts_new_ones() {
        let mut lines = Lines::from("held");
        // Cursor between "he" and "ld" -- pasting "llo\nbig wor" should
        // give two rows: "hello" and "big world".
        let cursor = Index2::new(0, 2);

        let result = splice_paste(&mut lines, cursor, "llo\nbig wor");

        assert_eq!(text_of(&lines), "hello\nbig world");
        assert_eq!(result, Index2::new(1, 6), "should land at the end of the pasted content on the new last row");
    }

    #[test]
    fn multi_line_paste_with_three_or_more_segments_inserts_every_middle_row() {
        let mut lines = Lines::from("ad");
        let cursor = Index2::new(0, 1);

        splice_paste(&mut lines, cursor, "b\nc");

        assert_eq!(text_of(&lines), "ab\ncd");
    }

    #[test]
    fn pasting_into_an_empty_buffer_works() {
        let mut lines = Lines::from("");
        let cursor = Index2::new(0, 0);

        let result = splice_paste(&mut lines, cursor, "hi\nthere");

        assert_eq!(text_of(&lines), "hi\nthere");
        assert_eq!(result, Index2::new(1, 4));
    }
}
