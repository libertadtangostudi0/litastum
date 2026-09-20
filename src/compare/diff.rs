use similar::{DiffOp, TextDiff};

/// What a display row in either pane actually shows -- drives both the
/// `Highlight` color (`ui/compare.rs::draw_compare`) and whether a
/// line-ending marker (`super::line_ending`) is even meaningful for it.
/// `Empty` is a padding row with no real source line at all -- see
/// `compute` below for why side-by-side alignment needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    Unchanged,
    Removed,
    Added,
    Empty,
}

/// One pane's own display rows, row-aligned with the *other* pane's
/// `DiffLines` -- `compute`'s own doc comment explains the alignment.
pub struct DiffLines {
    pub lines: Vec<String>,
    pub kinds: Vec<DiffLineKind>,
    /// This row's own line index in the *original* (pre-diff, pre-
    /// padding) file text, or `None` for an `Empty` padding row with no
    /// real source line at all -- `similar`'s own `DiffOp` variants
    /// already carry `old_index`/`new_index` directly, so this is just
    /// those values threaded through rather than a separately tracked
    /// counter. Used by `state.rs::ComparePane` to look up each
    /// displayed line's own real line-ending (`super::line_ending::detect`
    /// works against the *original* file text, not the diff's own
    /// padded/reordered display rows).
    pub source_index: Vec<Option<usize>>,
}

/// Line-level diff of `left_text`/`right_text`, producing two
/// **row-aligned** `DiffLines` -- row `i` on the left and row `i` on
/// the right are always meant to sit next to each other on screen, the
/// same "keep both sides in lockstep" convention every side-by-side
/// diff view uses (GitHub's own included). `similar::TextDiff`'s own
/// `Equal`/`Delete`/`Insert`/`Replace` ops don't naturally line up this
/// way on their own -- a `Delete` of 3 lines has nothing on the right
/// to sit next to, and a `Replace` of 2 old lines for 5 new ones is
/// naturally lopsided -- so the shorter side of any `Delete`/`Insert`/
/// `Replace` block is padded out with `DiffLineKind::Empty` rows to
/// match the longer side's own row count, keeping both `DiffLines`
/// exactly the same length and both panes' shared vertical scroll
/// position meaningful.
///
/// Each line has its own trailing `\n`/`\r\n` stripped (`similar`'s own
/// line splitting keeps it, since it also drives its unrelated unified-
/// diff writer) -- this view renders one line per row already, so the
/// terminator would otherwise show up as a literal, highlighted blank
/// cell at the end of every single line.
pub fn compute(left_text: &str, right_text: &str) -> (DiffLines, DiffLines) {
    let diff = TextDiff::from_lines(left_text, right_text);
    let mut left = DiffLines { lines: Vec::new(), kinds: Vec::new(), source_index: Vec::new() };
    let mut right = DiffLines { lines: Vec::new(), kinds: Vec::new(), source_index: Vec::new() };

    for op in diff.ops() {
        match *op {
            DiffOp::Equal { old_index, new_index, len } => {
                for i in 0..len {
                    left.lines.push(strip_line_ending(diff.old_slices()[old_index + i]));
                    left.kinds.push(DiffLineKind::Unchanged);
                    left.source_index.push(Some(old_index + i));
                    right.lines.push(strip_line_ending(diff.new_slices()[new_index + i]));
                    right.kinds.push(DiffLineKind::Unchanged);
                    right.source_index.push(Some(new_index + i));
                }
            }
            DiffOp::Delete { old_index, old_len, .. } => {
                for i in 0..old_len {
                    left.lines.push(strip_line_ending(diff.old_slices()[old_index + i]));
                    left.kinds.push(DiffLineKind::Removed);
                    left.source_index.push(Some(old_index + i));
                    right.lines.push(String::new());
                    right.kinds.push(DiffLineKind::Empty);
                    right.source_index.push(None);
                }
            }
            DiffOp::Insert { new_index, new_len, .. } => {
                for i in 0..new_len {
                    left.lines.push(String::new());
                    left.kinds.push(DiffLineKind::Empty);
                    left.source_index.push(None);
                    right.lines.push(strip_line_ending(diff.new_slices()[new_index + i]));
                    right.kinds.push(DiffLineKind::Added);
                    right.source_index.push(Some(new_index + i));
                }
            }
            DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                let rows = old_len.max(new_len);
                for i in 0..rows {
                    if i < old_len {
                        left.lines.push(strip_line_ending(diff.old_slices()[old_index + i]));
                        left.kinds.push(DiffLineKind::Removed);
                        left.source_index.push(Some(old_index + i));
                    } else {
                        left.lines.push(String::new());
                        left.kinds.push(DiffLineKind::Empty);
                        left.source_index.push(None);
                    }
                    if i < new_len {
                        right.lines.push(strip_line_ending(diff.new_slices()[new_index + i]));
                        right.kinds.push(DiffLineKind::Added);
                        right.source_index.push(Some(new_index + i));
                    } else {
                        right.lines.push(String::new());
                        right.kinds.push(DiffLineKind::Empty);
                        right.source_index.push(None);
                    }
                }
            }
        }
    }

    (left, right)
}

fn strip_line_ending(line: &str) -> String {
    line.trim_end_matches(['\n', '\r']).to_string()
}

/// The row of the next changed (non-`Unchanged`, non-`Empty`) line at
/// or after `from` -- drives "jump to next diff hunk"
/// (`state.rs::CompareState::jump_to_next_hunk`). Only one side's own
/// `kinds` needs checking: `compute`'s own row-alignment guarantee
/// means a row is `Unchanged`/`Empty` on both sides together, or
/// `Removed`/`Added` on (at least) one of them together -- never
/// "changed on the left but unchanged on the right" for the same row.
pub fn next_changed_row(kinds: &[DiffLineKind], from: usize) -> Option<usize> {
    kinds.iter().enumerate().skip(from).find(|(_, kind)| !matches!(kind, DiffLineKind::Unchanged)).map(|(row, _)| row)
}

/// The row of the previous changed line strictly before `from` -- the
/// other half of `next_changed_row`, for "jump to previous diff hunk."
pub fn previous_changed_row(kinds: &[DiffLineKind], from: usize) -> Option<usize> {
    kinds[..from.min(kinds.len())].iter().enumerate().rev().find(|(_, kind)| !matches!(kind, DiffLineKind::Unchanged)).map(|(row, _)| row)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_files_produce_only_unchanged_rows() {
        let (left, right) = compute("a\nb\nc\n", "a\nb\nc\n");
        assert_eq!(left.lines, vec!["a", "b", "c"]);
        assert_eq!(right.lines, vec!["a", "b", "c"]);
        assert!(left.kinds.iter().all(|k| *k == DiffLineKind::Unchanged));
        assert!(right.kinds.iter().all(|k| *k == DiffLineKind::Unchanged));
    }

    #[test]
    fn a_removed_line_pads_the_right_side_with_an_empty_row() {
        let (left, right) = compute("a\nb\nc\n", "a\nc\n");
        assert_eq!(left.lines, vec!["a", "b", "c"]);
        assert_eq!(left.kinds, vec![DiffLineKind::Unchanged, DiffLineKind::Removed, DiffLineKind::Unchanged]);
        assert_eq!(right.lines, vec!["a", "", "c"]);
        assert_eq!(right.kinds, vec![DiffLineKind::Unchanged, DiffLineKind::Empty, DiffLineKind::Unchanged]);
    }

    /// Regression coverage: `source_index` should point back at each
    /// display row's own real line in the *original* file, `None` only
    /// for `Empty` padding rows -- this is what lets a line-ending
    /// marker (`super::line_ending::detect`, which works against the
    /// original file text) be looked up for the right displayed line.
    #[test]
    fn source_index_maps_each_real_row_back_to_the_original_file_and_none_for_padding() {
        let (left, right) = compute("a\nb\nc\n", "a\nc\n");
        assert_eq!(left.source_index, vec![Some(0), Some(1), Some(2)], "left keeps every original line, including the removed one");
        assert_eq!(right.source_index, vec![Some(0), None, Some(1)], "right's padding row has no real source line");
    }

    #[test]
    fn an_added_line_pads_the_left_side_with_an_empty_row() {
        let (left, right) = compute("a\nc\n", "a\nb\nc\n");
        assert_eq!(left.lines, vec!["a", "", "c"]);
        assert_eq!(left.kinds, vec![DiffLineKind::Unchanged, DiffLineKind::Empty, DiffLineKind::Unchanged]);
        assert_eq!(right.lines, vec!["a", "b", "c"]);
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
        assert_eq!(left.lines.len(), right.lines.len());
        assert_eq!(left.lines, vec!["x", "", ""]);
        assert_eq!(left.kinds, vec![DiffLineKind::Removed, DiffLineKind::Empty, DiffLineKind::Empty]);
        assert_eq!(right.lines, vec!["one", "two", "three"]);
        assert!(right.kinds.iter().all(|k| *k == DiffLineKind::Added));
    }

    #[test]
    fn strips_both_lf_and_crlf_line_endings() {
        let (left, _) = compute("a\r\nb\n", "a\r\nb\n");
        assert_eq!(left.lines, vec!["a", "b"], "neither \\r\\n nor \\n should survive into the displayed line");
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
}
