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
    /// This row's own line index in the *live* buffer text `compute`
    /// was given, or `None` for an `Empty` padding row with no real
    /// source line at all. `similar`'s own `DiffOp` variants already
    /// carry `old_index`/`new_index` directly, so this is just those
    /// values threaded through rather than a separately tracked
    /// counter. Used both to paint a real editor row's own `Highlight`
    /// (`ui/compare.rs`) and, via `map_real_row` below, to keep the
    /// unfocused pane's viewport diff-aligned with the focused one.
    pub source_index: Vec<Option<usize>>,
}

/// Line-level diff of `left_text`/`right_text`, producing two
/// **row-aligned** `DiffLines` -- row `i` on the left and row `i` on
/// the right are always meant to line up on screen, the same
/// "keep both sides in lockstep" convention every side-by-side diff
/// view uses (GitHub's own included). `similar::TextDiff`'s own
/// `Equal`/`Delete`/`Insert`/`Replace` ops don't naturally line up this
/// way on their own -- a `Delete` of 3 lines has nothing on the right
/// to sit next to, and a `Replace` of 2 old lines for 5 new ones is
/// naturally lopsided -- so the shorter side of any `Delete`/`Insert`/
/// `Replace` block is padded out with `DiffLineKind::Empty` rows to
/// match the longer side's own row count.
///
/// Called fresh every frame against each pane's own live `Editor::text()`
/// (not a one-time snapshot) -- the two editable panes this drives
/// (`compare::CompareState`) can genuinely diverge from what was on disk
/// at open time, and the red/green highlighting needs to track that.
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

/// The first row of the next diff *hunk* -- a whole contiguous block of
/// changed lines, not just the next individual changed line -- at or
/// after `from`. Drives `F7`/`F8`/`Ctrl+Down` ("jump to next diff hunk",
/// `state.rs::CompareState::jump_to_next_hunk`).
///
/// Reported directly: a multi-line hunk (several consecutive
/// `Removed`/`Added` rows) made `F8` stop on every single line inside
/// it before finally moving on to the next real hunk, since the
/// original implementation called `next_changed_row` straight from
/// `cursor.row + 1` -- the row right after the cursor is still part of
/// the *same* hunk for anything longer than one line, so that was
/// always "the next changed row," never "the next hunk." Fixed by
/// skipping past `from`'s own hunk first (if it's sitting inside one)
/// before searching for the next changed row at all -- what's found
/// after that skip is guaranteed to belong to a different, later hunk.
pub fn next_hunk_start(kinds: &[DiffLineKind], from: usize) -> Option<usize> {
    let mut i = from;
    while i < kinds.len() && matches!(kinds[i], DiffLineKind::Removed | DiffLineKind::Added) {
        i += 1;
    }
    next_changed_row(kinds, i)
}

/// The first row of the previous diff hunk, strictly before whichever
/// hunk `from` itself sits inside (or before `from` outright, if it
/// isn't currently inside one) -- the other half of `next_hunk_start`,
/// same "skip the whole current block, not just one line of it" fix.
/// Lands on that previous hunk's own *first* row, not merely the
/// nearest changed line before `from` (which would land on its *last*
/// row instead, backward through a multi-line hunk one line at a time
/// -- the same bug `next_hunk_start` fixes, mirrored for this
/// direction): finds the nearest changed row before the current hunk,
/// then walks that row's own run back to where it starts.
pub fn previous_hunk_start(kinds: &[DiffLineKind], from: usize) -> Option<usize> {
    let boundary = if from < kinds.len() && matches!(kinds[from], DiffLineKind::Removed | DiffLineKind::Added) {
        hunk_start_at(kinds, from)
    } else {
        from
    };
    let last_row_of_previous_hunk = previous_changed_row(kinds, boundary)?;
    Some(hunk_start_at(kinds, last_row_of_previous_hunk))
}

/// Given the real row `from_real_row` currently at the top of the
/// *focused* pane's viewport, finds the row on the *other* side that
/// should sit at the top of its own viewport to stay diff-aligned --
/// used every frame to drive `Editor::set_viewport_top_row` on whichever
/// pane doesn't currently have focus (`ui/compare.rs::draw_compare`).
///
/// Walks `from_source_index` to find the diff row that real row landed
/// on, then walks `to_source_index` forward from there for the nearest
/// row with real content on the other side -- forward, not nearest in
/// either direction, so that scrolling to the top of an added/removed
/// block on the focused side aligns the other pane with the *start* of
/// that same block (or whatever real content immediately follows it),
/// matching how GitHub/VS Code-style diff views keep a change and its
/// counterpart-or-gap level with each other. Falls back to row `0` if
/// `from_real_row` isn't found at all (shouldn't happen for a valid
/// pair of `DiffLines` computed from the same `compute` call) or if
/// nothing on the other side has real content at or after that point
/// (the other side's remaining real content is all above the fold --
/// row `0` is as reasonable a fallback as any).
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
