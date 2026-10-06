use std::sync::atomic::{AtomicU64, Ordering};

use edtui::{Lines, RowIndex};

use super::super::word_highlight::{has_pathologically_long_line, MAX_HIGHLIGHTED_LINE_LEN};
use super::Editor;

/// A number for the buffer's content, new after every change and unique
/// across editors (`Editor::revision`): what Compare and the conflict
/// resolver key their diffs on.
pub(super) fn next_revision() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}


/// Where the buffer differs from what was loaded or saved -- `is_dirty`
/// without comparing the whole buffer after every keystroke, which cost
/// ~37 ms on a 300k-line file even in a release build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Differing {
    Nowhere,
    /// Only this row (same row count).
    Row(usize),
    /// Somewhere else, or in several places.
    Elsewhere,
}

/// `differing` after an edit changed only `row`, which now matches the
/// saved row (`same`) or not. Exact while at most one row differs, so
/// typing a character and deleting it again is clean, as before.
pub(super) fn after_row_edit(differing: Differing, row: usize, same: bool) -> Differing {
    match (differing, same) {
        (Differing::Nowhere, true) => Differing::Nowhere,
        (Differing::Nowhere, false) => Differing::Row(row),
        (Differing::Row(changed), same) if changed == row => {
            if same {
                Differing::Nowhere
            } else {
                Differing::Row(row)
            }
        }
        (Differing::Row(changed), true) => Differing::Row(changed),
        (Differing::Row(_), false) | (Differing::Elsewhere, _) => Differing::Elsewhere,
    }
}


/// What was loaded or last saved, as one hash per row: enough to tell
/// whether the buffer still matches it, without a second copy of the
/// whole buffer in memory (4 bytes a character -- ~200 MB for a 53 MB
/// file). Taken only before the first edit (`Editor::remember_saved_rows`),
/// so opening a file to read it costs nothing extra.
pub(super) struct SavedRows {
    hashes: Vec<u64>,
}

impl SavedRows {
    pub(super) fn of(lines: &Lines) -> Self {
        Self { hashes: (0..lines.len()).map(|row| row_hash(lines.get(RowIndex::new(row)).map_or(&[], Vec::as_slice))).collect() }
    }

    pub(super) fn matches(&self, lines: &Lines) -> bool {
        lines.len() == self.hashes.len() && (0..lines.len()).all(|row| self.row_matches(row, lines.get(RowIndex::new(row)).map_or(&[], Vec::as_slice)))
    }

    pub(super) fn row_matches(&self, row: usize, content: &[char]) -> bool {
        self.hashes.get(row) == Some(&row_hash(content))
    }

    pub(super) fn len(&self) -> usize {
        self.hashes.len()
    }
}


/// A fast 64-bit hash of a row (the `FxHash` mix): a collision would only
/// make an edited row look unchanged, odds of about 1 in 2^64. (The
/// standard library's hasher, called per character, was 3x slower in a
/// debug build.)
fn row_hash(row: &[char]) -> u64 {
    const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;
    let start = (row.len() as u64).wrapping_mul(SEED);
    row.iter().fold(start, |hash, &c| (hash.rotate_left(5) ^ u64::from(u32::from(c))).wrapping_mul(SEED))
}


impl Editor {
    /// Hashes the rows as loaded or saved, once, just before the first
    /// edit since -- the buffer still matches the file at that moment.
    pub(super) fn remember_saved_rows(&mut self) {
        if self.saved.is_none() {
            self.saved = Some(SavedRows::of(&self.state.lines));
        }
    }

    /// Everything that has to follow a change anywhere in the buffer
    /// (Enter, paste, deleting a selection, a whole-buffer undo): `dirty`
    /// by a full comparison, the long-line check, and the `Ctrl+F` box's
    /// matches if it's open -- one place, so no mutation path forgets one.
    pub(super) fn buffer_changed(&mut self) {
        // No saved rows means an edit skipped `remember_saved_rows`:
        // nothing to compare with, so dirty to be safe.
        let clean = self.saved.as_ref().is_some_and(|saved| saved.matches(&self.state.lines));
        self.differing = if clean { Differing::Nowhere } else { Differing::Elsewhere };
        self.has_long_line = has_pathologically_long_line(&self.state.lines);
        self.revision = next_revision();
        self.refresh_search_matches();
    }

    /// The same after an edit that changed only `row`: O(row length),
    /// not O(buffer). Recomputes the long-line check in full only when
    /// this row might have been the long one.
    pub(super) fn row_changed(&mut self, row: usize) {
        let current = self.state.lines.get(RowIndex::new(row));
        let same = self
            .saved
            .as_ref()
            .is_some_and(|saved| self.state.lines.len() == saved.len() && saved.row_matches(row, current.map_or(&[], Vec::as_slice)));
        self.differing = after_row_edit(self.differing, row, same);
        self.revision = next_revision();
        if current.is_some_and(|current| current.len() > MAX_HIGHLIGHTED_LINE_LEN) {
            self.has_long_line = true;
        } else if self.has_long_line {
            self.has_long_line = has_pathologically_long_line(&self.state.lines);
        }
        self.refresh_search_matches();
    }
}


/// The buffer as text, rows joined by `\n` -- what `String::from(Lines)`
/// gives, without cloning the buffer first or formatting it a character
/// at a time (`Display`, ~1.3 s for 300k lines in a debug build).
pub(super) fn lines_to_string(lines: &Lines) -> String {
    let rows = lines.len();
    let capacity = (0..rows).filter_map(|row| lines.get(RowIndex::new(row))).map(|row| row.len() + 1).sum();
    let mut text = String::with_capacity(capacity);
    for row in 0..rows {
        if row > 0 {
            text.push('\n');
        }
        if let Some(chars) = lines.get(RowIndex::new(row)) {
            text.extend(chars.iter());
        }
    }
    text
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_and_deleting_it_again_is_clean() {
        let typed = after_row_edit(Differing::Nowhere, 3, false);
        assert_eq!(typed, Differing::Row(3));
        assert_eq!(after_row_edit(typed, 3, true), Differing::Nowhere);
    }

    #[test]
    fn two_changed_rows_stay_dirty_whatever_happens_to_one() {
        let two = after_row_edit(Differing::Row(1), 2, false);
        assert_eq!(two, Differing::Elsewhere);
        assert_eq!(after_row_edit(two, 2, true), Differing::Elsewhere, "row 1 still differs");
        assert_eq!(after_row_edit(Differing::Row(1), 2, true), Differing::Row(1), "row 2 untouched in effect");
    }

    #[test]
    fn saved_rows_tell_a_changed_row_and_a_changed_row_count() {
        let saved = SavedRows::of(&Lines::from("ab\ncd"));
        assert!(saved.matches(&Lines::from("ab\ncd")));
        assert!(!saved.matches(&Lines::from("ab\ncx")));
        assert!(!saved.matches(&Lines::from("ab\ncd\n")), "one more (empty) row");
        assert!(saved.row_matches(1, &['c', 'd']));
        assert!(!saved.row_matches(1, &['d', 'c']), "order matters");
        assert!(!saved.row_matches(5, &[]), "no such row");
    }

    #[test]
    fn lines_to_string_matches_edtuis_own_conversion() {
        for text in ["", "a", "a\nb", "a\nb\n", "\n\n", "\u{0436}\u{1F600}\nx"] {
            let lines = Lines::from(text);
            assert_eq!(lines_to_string(&lines), String::from(lines.clone()), "{text:?}");
        }
    }
}
