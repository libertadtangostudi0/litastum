//! Cursor-aware text editing for a single-line field — currently just
//! the F5/F6/Shift+F6 destination path (`app::PendingTransfer`).
//!
//! Deliberately a separate module from `command_line.rs`, not a
//! generalization of it: the always-live command line keeps its
//! append/backspace-only, no-cursor-movement scope cut on purpose
//! (arrow keys there are needed for panel navigation even while typing
//! — see `.claude/rules/litastum-command-line.md`). The transfer
//! prompt is a modal popup with no panel navigation happening under
//! it, so arrows are free to mean "move within the text" there, and a
//! real cursor is what makes editing a filename in the middle of a
//! full path (the actual `Shift+F6` rename use case) usable at all.
//!
//! Positions are character indices (not byte offsets) into the
//! `String`, so multi-byte UTF-8 filenames don't panic on a split at a
//! non-boundary; `byte_index` is the one place that converts between
//! the two.

/// Byte offset of the `char_idx`-th character in `text` (or `text`'s
/// full length, for `char_idx == text.chars().count()` — the
/// end-of-string cursor position, which has no character of its own).
fn byte_index(text: &str, char_idx: usize) -> usize {
    text.char_indices().nth(char_idx).map(|(b, _)| b).unwrap_or(text.len())
}


/// Inserts `c` at `*cursor` and advances the cursor past it.
pub fn insert_char(text: &mut String, cursor: &mut usize, c: char) {
    let byte = byte_index(text, *cursor);
    text.insert(byte, c);
    *cursor += 1;
}


/// Deletes the character just before `*cursor` (a no-op at the start).
pub fn backspace(text: &mut String, cursor: &mut usize) {
    if *cursor == 0 {
        return;
    }
    let start = byte_index(text, *cursor - 1);
    let end = byte_index(text, *cursor);
    text.replace_range(start..end, "");
    *cursor -= 1;
}


/// Deletes the character at `*cursor` (a no-op at the end); the cursor
/// itself doesn't move, matching a normal editor's `Delete` key.
pub fn delete_forward(text: &mut String, cursor: &mut usize) {
    let len = text.chars().count();
    if *cursor >= len {
        return;
    }
    let start = byte_index(text, *cursor);
    let end = byte_index(text, *cursor + 1);
    text.replace_range(start..end, "");
}


/// Moves the cursor one character left, clamped at the start.
pub fn move_left(cursor: &mut usize) {
    *cursor = cursor.saturating_sub(1);
}


/// Moves the cursor one character right, clamped at the end.
pub fn move_right(text: &str, cursor: &mut usize) {
    let len = text.chars().count();
    if *cursor < len {
        *cursor += 1;
    }
}


pub fn move_home(cursor: &mut usize) {
    *cursor = 0;
}


pub fn move_end(text: &str, cursor: &mut usize) {
    *cursor = text.chars().count();
}


fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}


/// `/` and `\` -- singled out from the generic "non-word character" run
/// below so each one is its own word-movement stop, not silently
/// chained together with an adjacent word into one jump. Reported
/// directly against a real path (`/branches/Features/DataExtraction...`):
/// with `/` treated the same as any other separator (spaces, dots, ...),
/// Ctrl+Right from right before a `/` jumped straight through it *and*
/// the whole next path segment in one press, so landing right after
/// just the `/` needed bouncing Ctrl+Right then Ctrl+Left. Deliberately
/// narrow (just these two characters, not every separator) so the
/// already-established "skip a punctuation run, then the following
/// word, in one press" behavior for everything else (spaces, dots, ...)
/// stays exactly as documented/tested below.
fn is_path_sep(c: char) -> bool {
    c == '/' || c == '\\'
}


/// Moves the cursor to the start of the previous word (`Ctrl+Left`) —
/// skips any run of non-word characters (spaces, punctuation, ...)
/// immediately to the left first, then the word itself, same as a
/// standard text editor's word-left. A `/` or `\` right before the
/// cursor is its own stop instead (`is_path_sep`'s own doc comment) —
/// checked first, before the general run-skip, so it doesn't get
/// swallowed into either side of it.
pub fn move_word_left(text: &str, cursor: &mut usize) {
    let chars: Vec<char> = text.chars().collect();
    let mut i = *cursor;

    if i > 0 && is_path_sep(chars[i - 1]) {
        *cursor = i - 1;
        return;
    }

    while i > 0 && !is_word_char(chars[i - 1]) && !is_path_sep(chars[i - 1]) {
        i -= 1;
    }
    while i > 0 && is_word_char(chars[i - 1]) {
        i -= 1;
    }
    *cursor = i;
}


/// Moves the cursor to the start of the next word (`Ctrl+Right`) —
/// mirror of `move_word_left`.
pub fn move_word_right(text: &str, cursor: &mut usize) {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = *cursor;

    if i < len && is_path_sep(chars[i]) {
        *cursor = i + 1;
        return;
    }

    while i < len && !is_word_char(chars[i]) && !is_path_sep(chars[i]) {
        i += 1;
    }
    while i < len && is_word_char(chars[i]) {
        i += 1;
    }
    *cursor = i;
}


// -- Selection (Shift+Left/Right) --------------------------------------
//
// A selection is `anchor..cursor` (order-independent — `selection_range`
// normalizes it): `anchor` is where Shift+arrow started, `cursor` is the
// live end that keeps moving. `None` means no selection. Kept as a
// separate `Option<usize>` on `PendingTransfer` rather than folded into
// `cursor` itself, since most of the plain movement/editing functions
// above have no notion of a selection at all and shouldn't need one.

/// The selected char-range, `start <= end`, regardless of which
/// direction the selection was extended in.
pub fn selection_range(anchor: usize, cursor: usize) -> (usize, usize) {
    if anchor <= cursor { (anchor, cursor) } else { (cursor, anchor) }
}


/// Removes chars `start..end` from `text` (char indices, `start <= end`).
pub fn delete_range(text: &mut String, start: usize, end: usize) {
    let start_b = byte_index(text, start);
    let end_b = byte_index(text, end);
    text.replace_range(start_b..end_b, "");
}


/// `Shift+Left`: starts a selection at the current cursor if none is
/// active yet, then moves the cursor (the selection's live end) left.
pub fn extend_selection_left(cursor: &mut usize, anchor: &mut Option<usize>) {
    if anchor.is_none() {
        *anchor = Some(*cursor);
    }
    move_left(cursor);
}


/// `Shift+Right` — mirror of `extend_selection_left`.
pub fn extend_selection_right(text: &str, cursor: &mut usize, anchor: &mut Option<usize>) {
    if anchor.is_none() {
        *anchor = Some(*cursor);
    }
    move_right(text, cursor);
}


/// `Ctrl+Shift+Left` — word-wise version of `extend_selection_left`,
/// same anchor-starting behavior, extending by a whole word
/// (`move_word_left`) instead of one character.
pub fn extend_selection_word_left(text: &str, cursor: &mut usize, anchor: &mut Option<usize>) {
    if anchor.is_none() {
        *anchor = Some(*cursor);
    }
    move_word_left(text, cursor);
}


/// `Ctrl+Shift+Right` — mirror of `extend_selection_word_left`.
pub fn extend_selection_word_right(text: &str, cursor: &mut usize, anchor: &mut Option<usize>) {
    if anchor.is_none() {
        *anchor = Some(*cursor);
    }
    move_word_right(text, cursor);
}


/// Plain `Left` with a selection active: collapses to the selection's
/// start instead of moving one more character, matching a standard
/// text editor. With no selection, just moves left as usual.
pub fn collapse_selection_left(cursor: &mut usize, anchor: &mut Option<usize>) {
    match anchor.take() {
        Some(a) => *cursor = (*cursor).min(a),
        None => move_left(cursor),
    }
}


/// Plain `Right` with a selection active — mirror of
/// `collapse_selection_left`, collapsing to the selection's end.
pub fn collapse_selection_right(text: &str, cursor: &mut usize, anchor: &mut Option<usize>) {
    match anchor.take() {
        Some(a) => *cursor = (*cursor).max(a),
        None => move_right(text, cursor),
    }
}


/// If a selection is active, deletes it, moves the cursor to where it
/// started, and clears `anchor` — returns `true`. Otherwise leaves
/// everything untouched and returns `false`, so callers can fall back
/// to their own single-character `backspace`/`delete_forward`.
pub fn delete_selection(text: &mut String, cursor: &mut usize, anchor: &mut Option<usize>) -> bool {
    let Some(a) = anchor.take() else {
        return false;
    };
    let (start, end) = selection_range(a, *cursor);
    delete_range(text, start, end);
    *cursor = start;
    true
}


#[cfg(test)]
mod tests {
    use super::*;

    mod editing_tests {
        use super::*;

    #[test]
    fn insert_char_at_cursor_not_just_at_the_end() {
        let mut text = "helo".to_string();
        let mut cursor = 3; // "hel|o"
        insert_char(&mut text, &mut cursor, 'l');
        assert_eq!(text, "hello");
        assert_eq!(cursor, 4);
    }

    #[test]
    fn insert_char_handles_multibyte_utf8() {
        let mut text = "café".to_string();
        let mut cursor = 4; // after the 4th char ('é'), the end
        insert_char(&mut text, &mut cursor, '!');
        assert_eq!(text, "café!");
    }

    #[test]
    fn backspace_removes_the_character_before_the_cursor() {
        let mut text = "hello".to_string();
        let mut cursor = 3; // "hel|lo"
        backspace(&mut text, &mut cursor);
        assert_eq!(text, "helo");
        assert_eq!(cursor, 2);
    }

    #[test]
    fn backspace_at_start_is_a_noop() {
        let mut text = "hello".to_string();
        let mut cursor = 0;
        backspace(&mut text, &mut cursor);
        assert_eq!(text, "hello");
        assert_eq!(cursor, 0);
    }

    #[test]
    fn delete_forward_removes_the_character_at_the_cursor_without_moving_it() {
        let mut text = "hello".to_string();
        let mut cursor = 1; // "h|ello"
        delete_forward(&mut text, &mut cursor);
        assert_eq!(text, "hllo");
        assert_eq!(cursor, 1);
    }

    #[test]
    fn delete_forward_at_end_is_a_noop() {
        let mut text = "hello".to_string();
        let mut cursor = 5;
        delete_forward(&mut text, &mut cursor);
        assert_eq!(text, "hello");
    }
    }

    mod movement_tests {
        use super::*;

    #[test]
    fn move_left_and_right_are_clamped() {
        let text = "ab";
        let mut cursor = 0;
        move_left(&mut cursor);
        assert_eq!(cursor, 0, "clamped at start");

        cursor = 2;
        move_right(text, &mut cursor);
        assert_eq!(cursor, 2, "clamped at end");
    }

    #[test]
    fn home_and_end_jump_to_the_edges() {
        let text = "hello";
        let mut cursor = 2;
        move_home(&mut cursor);
        assert_eq!(cursor, 0);
        move_end(text, &mut cursor);
        assert_eq!(cursor, 5);
    }

    #[test]
    fn move_word_left_skips_separators_then_the_word() {
        let text = "C:/Users/name/file.txt";
        let mut cursor = text.chars().count(); // end, right after "file.txt"
        move_word_left(text, &mut cursor);
        // Lands at the start of "txt" -- '.' isn't a word char, so it's
        // skipped as a separator, same as a real editor's Ctrl+Left.
        assert_eq!(&text[cursor..], "txt");
    }

    #[test]
    fn move_word_right_skips_the_word_then_separators() {
        let text = "name/file";
        let mut cursor = 0;
        move_word_right(text, &mut cursor);
        assert_eq!(&text[cursor..], "/file");
    }

    /// Regression test for the real reported path: crossing a `/`
    /// needs its own Ctrl+Right press, not silently chained into the
    /// following path segment the way a space or `.` would be.
    #[test]
    fn move_word_right_stops_right_after_a_path_separator() {
        let text = "/branches/features";
        let mut cursor = 0;

        move_word_right(text, &mut cursor); // over the leading "/"
        assert_eq!(cursor, 1);

        move_word_right(text, &mut cursor); // over "branches"
        assert_eq!(&text[cursor..], "/features");

        move_word_right(text, &mut cursor); // over that "/"
        assert_eq!(&text[cursor..], "features");
    }

    #[test]
    fn move_word_left_stops_right_before_a_path_separator() {
        let text = "/branches/features";
        let mut cursor = text.chars().count();

        move_word_left(text, &mut cursor); // back over "features"
        assert_eq!(&text[cursor..], "features");

        move_word_left(text, &mut cursor); // back over that "/"
        assert_eq!(&text[cursor..], "/features");

        move_word_left(text, &mut cursor); // back over "branches"
        assert_eq!(&text[cursor..], "branches/features");

        move_word_left(text, &mut cursor); // back over the leading "/"
        assert_eq!(cursor, 0);
    }

    /// Backslashes (Windows paths) get the same per-character stop as
    /// forward slashes.
    #[test]
    fn move_word_right_stops_right_after_a_backslash() {
        let text = r"C:\Users\name";
        let mut cursor = 2; // right after "C:"

        move_word_right(text, &mut cursor); // over the "\"
        assert_eq!(cursor, 3);
    }

    #[test]
    fn word_movement_is_clamped_at_the_edges() {
        let text = "word";
        let mut cursor = 0;
        move_word_left(text, &mut cursor);
        assert_eq!(cursor, 0);

        cursor = text.chars().count();
        move_word_right(text, &mut cursor);
        assert_eq!(cursor, text.chars().count());
    }
    }

    mod selection_tests {
        use super::*;

    #[test]
    fn shift_right_starts_and_extends_a_selection() {
        let text = "hello";
        let mut cursor = 1;
        let mut anchor = None;
        extend_selection_right(text, &mut cursor, &mut anchor);
        extend_selection_right(text, &mut cursor, &mut anchor);
        assert_eq!(anchor, Some(1));
        assert_eq!(cursor, 3);
        assert_eq!(selection_range(anchor.unwrap(), cursor), (1, 3));
    }

    #[test]
    fn shift_left_from_a_shift_right_selection_shrinks_it_back() {
        // Selecting right then left past the anchor should flip which
        // side is the selection start -- selection_range must still
        // normalize it, not assume anchor <= cursor.
        let text = "hello";
        let mut cursor = 2;
        let mut anchor = None;
        extend_selection_right(text, &mut cursor, &mut anchor);
        extend_selection_left(&mut cursor, &mut anchor);
        extend_selection_left(&mut cursor, &mut anchor);
        assert_eq!(selection_range(anchor.unwrap(), cursor), (1, 2));
    }

    #[test]
    fn ctrl_shift_right_extends_selection_by_a_whole_word() {
        let text = "svn info";
        let mut cursor = 0;
        let mut anchor = None;
        extend_selection_word_right(text, &mut cursor, &mut anchor);
        assert_eq!(anchor, Some(0));
        assert_eq!(cursor, 3, "should land right after \"svn\", before the space");
        assert_eq!(selection_range(anchor.unwrap(), cursor), (0, 3));
    }

    #[test]
    fn ctrl_shift_left_extends_selection_by_a_whole_word() {
        let text = "svn info";
        let mut cursor = 8; // end
        let mut anchor = None;
        extend_selection_word_left(text, &mut cursor, &mut anchor);
        assert_eq!(anchor, Some(8));
        assert_eq!(cursor, 4, "should land right at the start of \"info\"");
    }

    #[test]
    fn plain_left_collapses_selection_to_its_start() {
        let mut cursor = 4;
        let mut anchor = Some(1);
        collapse_selection_left(&mut cursor, &mut anchor);
        assert_eq!(cursor, 1);
        assert_eq!(anchor, None, "selection should be cleared, not just moved");
    }

    #[test]
    fn plain_right_collapses_selection_to_its_end() {
        let text = "hello";
        let mut cursor = 1;
        let mut anchor = Some(4);
        collapse_selection_right(text, &mut cursor, &mut anchor);
        assert_eq!(cursor, 4);
        assert_eq!(anchor, None);
    }

    #[test]
    fn plain_left_with_no_selection_just_moves_left() {
        let mut cursor = 2;
        let mut anchor = None;
        collapse_selection_left(&mut cursor, &mut anchor);
        assert_eq!(cursor, 1);
    }

    #[test]
    fn delete_selection_removes_the_selected_range_and_clears_it() {
        let mut text = "hello world".to_string();
        let mut cursor = 5; // "hello| world" -- selected "hello" via anchor 0
        let mut anchor = Some(0);
        let deleted = delete_selection(&mut text, &mut cursor, &mut anchor);
        assert!(deleted);
        assert_eq!(text, " world");
        assert_eq!(cursor, 0);
        assert_eq!(anchor, None);
    }

    #[test]
    fn delete_selection_is_a_noop_and_returns_false_with_nothing_selected() {
        let mut text = "hello".to_string();
        let mut cursor = 2;
        let mut anchor = None;
        let deleted = delete_selection(&mut text, &mut cursor, &mut anchor);
        assert!(!deleted);
        assert_eq!(text, "hello");
        assert_eq!(cursor, 2);
    }
    }
}
