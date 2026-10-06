use std::ops::Range;

use similar::{DiffOp, TextDiff};

/// What a diff row actually is on one side -- drives the `Highlight`
/// color painted over that row's real editor line (`ui/compare.rs`).
/// `Empty` is a padding row with no real source line on *this* side at
/// all -- see `compute` below for why row alignment needs it, even
/// though nothing renders it directly any more (`Editor` owns and
/// renders its own real buffer; this module only classifies rows and
/// hands back real-line indices via `source_index`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    Unchanged,
    Removed,
    Added,
    Empty,
}

/// One side's own row classification, row-aligned with the *other*
/// side's `DiffLines` -- `compute`'s own doc comment explains the
/// alignment.
pub struct DiffLines {
    pub kinds: Vec<DiffLineKind>,
    /// Each row's line index in the live text, `None` for an `Empty` padding
    /// row (straight from `similar`'s `old_index`/`new_index`). Used for the
    /// row highlights and `map_real_row`.
    pub source_index: Vec<Option<usize>>,
}

/// Line diff producing two row-aligned `DiffLines` (row `i` on each side
/// shown level): the shorter side of each `Delete`/`Insert`/`Replace`
/// block is padded with `Empty` rows. Called every frame on the live
/// text.
pub fn compute(left_text: &str, right_text: &str) -> (DiffLines, DiffLines) {
    let diff = TextDiff::from_lines(left_text, right_text);
    let mut left = DiffLines { kinds: Vec::new(), source_index: Vec::new() };
    let mut right = DiffLines { kinds: Vec::new(), source_index: Vec::new() };

    for op in diff.ops() {
        match *op {
            DiffOp::Equal { old_index, new_index, len } => {
                for i in 0..len {
                    left.kinds.push(DiffLineKind::Unchanged);
                    left.source_index.push(Some(old_index + i));
                    right.kinds.push(DiffLineKind::Unchanged);
                    right.source_index.push(Some(new_index + i));
                }
            }
            DiffOp::Delete { old_index, old_len, .. } => {
                for i in 0..old_len {
                    left.kinds.push(DiffLineKind::Removed);
                    left.source_index.push(Some(old_index + i));
                    right.kinds.push(DiffLineKind::Empty);
                    right.source_index.push(None);
                }
            }
            DiffOp::Insert { new_index, new_len, .. } => {
                for i in 0..new_len {
                    left.kinds.push(DiffLineKind::Empty);
                    left.source_index.push(None);
                    right.kinds.push(DiffLineKind::Added);
                    right.source_index.push(Some(new_index + i));
                }
            }
            DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                let rows = old_len.max(new_len);
                for i in 0..rows {
                    if i < old_len {
                        left.kinds.push(DiffLineKind::Removed);
                        left.source_index.push(Some(old_index + i));
                    } else {
                        left.kinds.push(DiffLineKind::Empty);
                        left.source_index.push(None);
                    }
                    if i < new_len {
                        right.kinds.push(DiffLineKind::Added);
                        right.source_index.push(Some(new_index + i));
                    } else {
                        right.kinds.push(DiffLineKind::Empty);
                        right.source_index.push(None);
                    }
                }
            }
        }
    }

    (left, right)
}

/// Lines longer than this (both together, in characters) are marked
/// changed as a whole rather than diffed character by character.
const MAX_INLINE_DIFF_CHARS: usize = 2000;

/// Unchanged stretches this short between two changes are counted as
/// changed, so a line reads as a few changed words rather than a speckle
/// of single characters.
const MERGE_GAP: usize = 2;


/// The characters that differ between a changed line (`old`, the left
/// side) and its counterpart (`new`): character ranges on each side, for
/// highlighting just the change rather than the whole line. Both whole
/// for a very long line.
pub fn inline_changes(old: &str, new: &str) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    let (old_len, new_len) = (old.chars().count(), new.chars().count());
    if old_len + new_len > MAX_INLINE_DIFF_CHARS {
        return (std::iter::once(0..old_len).collect(), std::iter::once(0..new_len).collect());
    }
    let diff = TextDiff::from_chars(old, new);
    let (mut old_changes, mut new_changes) = (Vec::new(), Vec::new());
    for op in diff.ops() {
        match *op {
            DiffOp::Equal { .. } => {}
            DiffOp::Delete { old_index, old_len, .. } => old_changes.push(old_index..old_index + old_len),
            DiffOp::Insert { new_index, new_len, .. } => new_changes.push(new_index..new_index + new_len),
            DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                old_changes.push(old_index..old_index + old_len);
                new_changes.push(new_index..new_index + new_len);
            }
        }
    }
    (merge_close(old_changes), merge_close(new_changes))
}


fn merge_close(ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in ranges.into_iter().filter(|range| !range.is_empty()) {
        match merged.last_mut() {
            Some(last) if range.start <= last.end + MERGE_GAP => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    merged
}


/// The row of the next changed line with real content on *this* side
/// (`Removed`/`Added`, never `Empty` padding -- there's nothing to jump
/// the cursor onto there) at or after `from`. A building block for
/// `next_hunk_start`/`previous_hunk_start` below, not used directly for
/// hunk navigation any more -- see those functions' own doc comments
/// for why "the next changed row" and "the next *hunk*" aren't the same
/// thing once a hunk spans more than one line.
pub fn next_changed_row(kinds: &[DiffLineKind], from: usize) -> Option<usize> {
    kinds.iter().enumerate().skip(from).find(|(_, kind)| matches!(kind, DiffLineKind::Removed | DiffLineKind::Added)).map(|(row, _)| row)
}

/// The row of the previous changed line with real content on this side,
/// strictly before `from` -- the other half of `next_changed_row`.
pub fn previous_changed_row(kinds: &[DiffLineKind], from: usize) -> Option<usize> {
    kinds[..from.min(kinds.len())]
        .iter()
        .enumerate()
        .rev()
        .find(|(_, kind)| matches!(kind, DiffLineKind::Removed | DiffLineKind::Added))
        .map(|(row, _)| row)
}

/// The first row of the contiguous changed run (`Removed`/`Added`) that
/// `row` itself sits inside -- walks backward while the row right
/// before it is still part of the same run. `row` is assumed to already
/// be a changed row; callers only ever reach this after checking that.
fn hunk_start_at(kinds: &[DiffLineKind], row: usize) -> usize {
    let mut start = row;
    while start > 0 && matches!(kinds[start - 1], DiffLineKind::Removed | DiffLineKind::Added) {
        start -= 1;
    }
    start
}

/// First row of the next hunk (a contiguous changed block) at or after
/// `from`, for `F7`/`F8`/`Ctrl+Down`. Skips `from`'s own hunk first --
/// searching from the next row stopped on every line of a multi-line
/// hunk. History: docs/history/compare.md.
pub fn next_hunk_start(kinds: &[DiffLineKind], from: usize) -> Option<usize> {
    let mut i = from;
    while i < kinds.len() && matches!(kinds[i], DiffLineKind::Removed | DiffLineKind::Added) {
        i += 1;
    }
    next_changed_row(kinds, i)
}

/// First row of the previous hunk, before the one `from` is in -- landing
/// on that hunk's first row, not its last, so `F7` doesn't step backward
/// through a hunk line by line (the mirror of `next_hunk_start`).
pub fn previous_hunk_start(kinds: &[DiffLineKind], from: usize) -> Option<usize> {
    let boundary = if from < kinds.len() && matches!(kinds[from], DiffLineKind::Removed | DiffLineKind::Added) {
        hunk_start_at(kinds, from)
    } else {
        from
    };
    let last_row_of_previous_hunk = previous_changed_row(kinds, boundary)?;
    Some(hunk_start_at(kinds, last_row_of_previous_hunk))
}

/// The real row each hunk starts on, on this side -- where `F8` stops.
/// An insertion on the other side is only padding here, not a hunk.
pub fn hunk_start_rows(diff: &DiffLines) -> Vec<usize> {
    let is_changed = |kind: &DiffLineKind| matches!(kind, DiffLineKind::Removed | DiffLineKind::Added);
    (0..diff.kinds.len())
        .filter(|&row| is_changed(&diff.kinds[row]) && (row == 0 || !is_changed(&diff.kinds[row - 1])))
        .filter_map(|row| diff.source_index[row])
        .collect()
}

/// The diff row holding real line `real_row` on this side, or the first
/// real line after it -- diff rows include `Empty` padding, so the two
/// numberings drift apart after every insertion on the other side.
/// `source_index.len()` past the last line.
pub fn diff_row_of(source_index: &[Option<usize>], real_row: usize) -> usize {
    source_index.iter().position(|row| row.is_some_and(|row| row >= real_row)).unwrap_or(source_index.len())
}

/// The other pane's real row to put at its top, given the focused
/// pane's top `from_real_row`: the diff row it lands on, then forward to
/// the nearest real content on the other side -- forward, so a changed
/// block and its gap start level. `0` if nothing is found.
pub fn map_real_row(from_source_index: &[Option<usize>], to_source_index: &[Option<usize>], from_real_row: usize) -> usize {
    let Some(diff_row) = from_source_index.iter().position(|row| *row == Some(from_real_row)) else {
        return 0;
    };
    to_source_index[diff_row..].iter().find_map(|row| *row).unwrap_or(0)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_files_produce_only_unchanged_rows() {
        let (left, right) = compute("a\nb\nc\n", "a\nb\nc\n");
        assert!(left.kinds.iter().all(|k| *k == DiffLineKind::Unchanged));
        assert!(right.kinds.iter().all(|k| *k == DiffLineKind::Unchanged));
    }

    #[test]
    fn a_removed_line_pads_the_right_side_with_an_empty_row() {
        let (left, right) = compute("a\nb\nc\n", "a\nc\n");
        assert_eq!(left.kinds, vec![DiffLineKind::Unchanged, DiffLineKind::Removed, DiffLineKind::Unchanged]);
        assert_eq!(right.kinds, vec![DiffLineKind::Unchanged, DiffLineKind::Empty, DiffLineKind::Unchanged]);
    }

    /// Regression coverage: `source_index` should point back at each
    /// row's own real line in the live buffer text, `None` only for
    /// `Empty` padding rows.
    #[test]
    fn source_index_maps_each_real_row_back_to_the_live_text_and_none_for_padding() {
        let (left, right) = compute("a\nb\nc\n", "a\nc\n");
        assert_eq!(left.source_index, vec![Some(0), Some(1), Some(2)], "left keeps every original line, including the removed one");
        assert_eq!(right.source_index, vec![Some(0), None, Some(1)], "right's padding row has no real source line");
    }

    #[test]
    fn an_added_line_pads_the_left_side_with_an_empty_row() {
        let (left, right) = compute("a\nc\n", "a\nb\nc\n");
        assert_eq!(left.kinds, vec![DiffLineKind::Unchanged, DiffLineKind::Empty, DiffLineKind::Unchanged]);
        assert_eq!(right.kinds, vec![DiffLineKind::Unchanged, DiffLineKind::Added, DiffLineKind::Unchanged]);
    }

    /// A replace block with an unequal number of old/new lines should
    /// still keep both sides the same total row count -- the shorter
    /// side (here, one old line replaced by three new ones) gets padded
    /// with `Empty` rows rather than leaving the two `DiffLines` at
    /// different lengths.
    #[test]
    fn a_replace_with_unequal_line_counts_keeps_both_sides_row_aligned() {
        let (left, right) = compute("x\n", "one\ntwo\nthree\n");
        assert_eq!(left.kinds.len(), right.kinds.len());
        assert_eq!(left.kinds, vec![DiffLineKind::Removed, DiffLineKind::Empty, DiffLineKind::Empty]);
        assert!(right.kinds.iter().all(|k| *k == DiffLineKind::Added));
    }

    #[test]
    fn hunk_start_rows_are_real_rows_and_skip_padding() {
        let (left, right) = compute("a\nb\nc\nd\ne\n", "a\nnew1\nnew2\nb\nc\nX\nY\ne\n");
        assert_eq!(hunk_start_rows(&left), [3], "\"d\"; the insertion is only padding on the left");
        assert_eq!(hunk_start_rows(&right), [1, 5], "the insertion, then \"X\" (one hunk with \"Y\")");
    }

    #[test]
    fn only_the_changed_characters_are_marked() {
        assert_eq!(inline_changes("#define X", "//#define X"), (vec![], vec![0..2]));
        assert_eq!(inline_changes("abc", "abd"), (vec![2..3], vec![2..3]));
    }

    #[test]
    fn close_changes_merge_into_one() {
        let (old, new) = inline_changes("a1b2c", "a9b8c");
        assert_eq!(old, vec![1..4], "1, b, 2: one stretch rather than two speckles");
        assert_eq!(new, vec![1..4]);
    }

    #[test]
    fn a_very_long_line_is_changed_as_a_whole() {
        let long = "x".repeat(MAX_INLINE_DIFF_CHARS);
        let (old, new) = inline_changes(&long, "y");
        assert_eq!((old, new), (vec![0..MAX_INLINE_DIFF_CHARS], vec![0..1]));
    }

    #[test]
    fn diff_row_of_skips_padding_rows() {
        let source_index = vec![Some(0), None, None, Some(1), Some(2)];
        assert_eq!(diff_row_of(&source_index, 0), 0);
        assert_eq!(diff_row_of(&source_index, 1), 3, "two padding rows sit before real line 1");
        assert_eq!(diff_row_of(&source_index, 2), 4);
        assert_eq!(diff_row_of(&source_index, 9), 5, "past the last line");
    }

    #[test]
    fn next_changed_row_skips_empty_padding_rows_too() {
        let kinds = vec![DiffLineKind::Removed, DiffLineKind::Empty, DiffLineKind::Empty, DiffLineKind::Unchanged];
        assert_eq!(next_changed_row(&kinds, 1), None, "the only real content on this side is row 0, already behind `from`");
    }

    #[test]
    fn next_changed_row_finds_the_first_change_at_or_after_from() {
        let kinds = vec![DiffLineKind::Unchanged, DiffLineKind::Removed, DiffLineKind::Unchanged, DiffLineKind::Added];
        assert_eq!(next_changed_row(&kinds, 0), Some(1));
        assert_eq!(next_changed_row(&kinds, 1), Some(1), "should include `from` itself, not just strictly after it");
        assert_eq!(next_changed_row(&kinds, 2), Some(3));
        assert_eq!(next_changed_row(&kinds, 4), None, "past the end of the list");
    }

    #[test]
    fn previous_changed_row_finds_the_last_change_strictly_before_from() {
        let kinds = vec![DiffLineKind::Unchanged, DiffLineKind::Removed, DiffLineKind::Unchanged, DiffLineKind::Added];
        assert_eq!(previous_changed_row(&kinds, 4), Some(3));
        assert_eq!(previous_changed_row(&kinds, 3), Some(1), "should not include `from` itself");
        assert_eq!(previous_changed_row(&kinds, 1), None);
        assert_eq!(previous_changed_row(&kinds, 0), None);
    }

    /// Regression coverage for the real report: from inside a two-row
    /// hunk (rows 1-2), `next_hunk_start` should skip straight to the
    /// next hunk's own first row (5), not stop at row 2 first the way
    /// plain `next_changed_row(kinds, from + 1)` used to.
    #[test]
    fn next_hunk_start_skips_the_rest_of_a_multi_line_hunk() {
        let kinds = vec![
            DiffLineKind::Unchanged,
            DiffLineKind::Removed,
            DiffLineKind::Removed,
            DiffLineKind::Unchanged,
            DiffLineKind::Unchanged,
            DiffLineKind::Added,
        ];
        assert_eq!(next_hunk_start(&kinds, 0), Some(1), "from before any hunk, lands on the first hunk's own start");
        assert_eq!(next_hunk_start(&kinds, 1), Some(5), "from the first row of the current hunk, skips past row 2 to the next hunk");
        assert_eq!(next_hunk_start(&kinds, 2), Some(5), "from the last row of the current hunk, same destination");
        assert_eq!(next_hunk_start(&kinds, 5), None, "no further hunk past the last one");
    }

    /// The other direction: from inside the second (one-row) hunk,
    /// `previous_hunk_start` should land on the earlier hunk's own
    /// *first* row (1), not its last row (2) the way plain
    /// `previous_changed_row` would.
    #[test]
    fn previous_hunk_start_lands_on_the_earlier_hunks_own_first_row() {
        let kinds = vec![
            DiffLineKind::Unchanged,
            DiffLineKind::Removed,
            DiffLineKind::Removed,
            DiffLineKind::Unchanged,
            DiffLineKind::Unchanged,
            DiffLineKind::Added,
        ];
        assert_eq!(previous_hunk_start(&kinds, 5), Some(1), "from the only row of the second hunk");
        assert_eq!(previous_hunk_start(&kinds, 2), None, "already inside the first hunk -- nothing earlier to find");
        assert_eq!(previous_hunk_start(&kinds, 1), None, "already on the first hunk's own start -- same no-op");
    }

    #[test]
    fn map_real_row_finds_the_matching_row_on_the_other_side() {
        let (left, right) = compute("a\nb\nc\n", "a\nb\nc\n");
        assert_eq!(map_real_row(&left.source_index, &right.source_index, 1), 1, "identical files -- rows map 1:1");
    }

    #[test]
    fn map_real_row_skips_forward_past_a_gap_with_no_real_content() {
        // left: a, b(removed), c -- right: a, c (no row 1 at all on the right)
        let (left, right) = compute("a\nb\nc\n", "a\nc\n");
        assert_eq!(map_real_row(&left.source_index, &right.source_index, 1), 1, "left's removed row 1 has no right counterpart -- lands on the next real row (right's own row 1, \"c\")");
    }

    #[test]
    fn map_real_row_falls_back_to_zero_when_nothing_real_follows_on_the_other_side() {
        let (left, right) = compute("a\nb\n", "a\nb\nc\n");
        assert_eq!(map_real_row(&right.source_index, &left.source_index, 2), 0, "right's added trailing row has nothing after it on the left");
    }
}
