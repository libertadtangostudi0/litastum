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
/// the cursor onto there) at or after `from`. Drives "jump to next diff
/// hunk" (`state.rs::CompareState::jump_to_next_hunk`) against whichever
/// side is currently focused.
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
