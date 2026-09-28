use super::*;
use ratatui::style::Color;

fn style() -> Style {
    Style::default().fg(Color::White).bg(Color::Blue)
}

#[test]
fn highlights_the_matching_close_from_the_opening_side() {
    let lines = Lines::from("fn main() {}");
    // Cursor on '(' at column 7 -- both it and the ')' at column 8
    // should be highlighted.
    let highlights = bracket_match_highlights(&lines, Index2::new(0, 7), style());

    assert_eq!(
        highlights,
        vec![Highlight::new(Index2::new(0, 7), Index2::new(0, 7), style()), Highlight::new(Index2::new(0, 8), Index2::new(0, 8), style())]
    );
}

#[test]
fn highlights_the_matching_open_from_the_closing_side() {
    let lines = Lines::from("fn main() {}");
    // Cursor on ')' at column 8 -- both it and the '(' at column 7
    // should be highlighted.
    let highlights = bracket_match_highlights(&lines, Index2::new(0, 8), style());

    assert_eq!(
        highlights,
        vec![Highlight::new(Index2::new(0, 8), Index2::new(0, 8), style()), Highlight::new(Index2::new(0, 7), Index2::new(0, 7), style())]
    );
}

/// Same "touching from the right" convention as `word_highlight`'s
/// own `word_at` -- the cursor sitting one column past a bracket
/// (the insert-mode append position, where the cell under the
/// cursor itself isn't a bracket at all) should still match it.
#[test]
fn matches_the_bracket_immediately_to_the_lefts_cursor_position() {
    let lines = Lines::from("(a)");
    // Cursor at column 3, one past the closing ')' -- there's no
    // character there at all, so this can only resolve via the
    // "cell to the left" fallback.
    let highlights = bracket_match_highlights(&lines, Index2::new(0, 3), style());

    assert_eq!(
        highlights,
        vec![Highlight::new(Index2::new(0, 2), Index2::new(0, 2), style()), Highlight::new(Index2::new(0, 0), Index2::new(0, 0), style())]
    );
}

#[test]
fn ignores_a_different_bracket_kind_while_nested() {
    let lines = Lines::from("([)]");
    // The '(' at column 0 must match the ')' at column 2, not the
    // '[' at column 1 -- confirms same-kind-only nesting depth.
    let highlights = bracket_match_highlights(&lines, Index2::new(0, 0), style());

    assert_eq!(
        highlights,
        vec![Highlight::new(Index2::new(0, 0), Index2::new(0, 0), style()), Highlight::new(Index2::new(0, 2), Index2::new(0, 2), style())]
    );
}

#[test]
fn skips_a_nested_same_kind_pair_to_find_the_real_match() {
    let lines = Lines::from("(a(b)c)");
    let highlights = bracket_match_highlights(&lines, Index2::new(0, 0), style());

    assert_eq!(
        highlights,
        vec![Highlight::new(Index2::new(0, 0), Index2::new(0, 0), style()), Highlight::new(Index2::new(0, 6), Index2::new(0, 6), style())]
    );
}

#[test]
fn matches_across_multiple_lines() {
    let lines = Lines::from("fn main() {\n    let x = 1;\n}");
    // Cursor on the '{' at the end of row 0.
    let highlights = bracket_match_highlights(&lines, Index2::new(0, 11), style());

    assert_eq!(
        highlights,
        vec![Highlight::new(Index2::new(0, 10), Index2::new(0, 10), style()), Highlight::new(Index2::new(2, 0), Index2::new(2, 0), style())]
    );
}

#[test]
fn matches_angle_brackets_in_a_generic_type() {
    let lines = Lines::from("Vec<Option<T>>");
    // Cursor on the outer '<' at column 3 -- must match the outer
    // '>' at column 13, not the inner one at column 12.
    let highlights = bracket_match_highlights(&lines, Index2::new(0, 3), style());

    assert_eq!(
        highlights,
        vec![Highlight::new(Index2::new(0, 3), Index2::new(0, 3), style()), Highlight::new(Index2::new(0, 13), Index2::new(0, 13), style())]
    );
}

/// Documents the known tradeoff (`PAIRS`'s own doc comment): with no
/// syntax awareness, a real comparison `<`/`>` is indistinguishable
/// from a generic/tag delimiter to this module's plain depth scan.
/// Not a bug to fix -- just pinning down what actually happens so a
/// future report isn't surprising.
#[test]
fn angle_brackets_can_mismatch_against_real_comparison_operators() {
    let lines = Lines::from("if a < b { c > d }");
    // The '<' at column 5 pairs with the '>' at column 13 by plain
    // depth counting, even though neither is really a generic/tag
    // delimiter here.
    let highlights = bracket_match_highlights(&lines, Index2::new(0, 5), style());

    assert_eq!(
        highlights,
        vec![Highlight::new(Index2::new(0, 5), Index2::new(0, 5), style()), Highlight::new(Index2::new(0, 13), Index2::new(0, 13), style())]
    );
}

#[test]
fn no_highlights_for_an_unmatched_bracket() {
    let lines = Lines::from("(a + b");
    let highlights = bracket_match_highlights(&lines, Index2::new(0, 0), style());

    assert!(highlights.is_empty());
}

#[test]
fn no_highlights_when_the_cursor_is_not_touching_a_bracket() {
    let lines = Lines::from("hello world");
    let highlights = bracket_match_highlights(&lines, Index2::new(0, 3), style());

    assert!(highlights.is_empty());
}

/// Regression guard for the explicit request: a plain identifier
/// character is never mistaken for a bracket, and this module never
/// reaches into `word_highlight`'s own matching at all -- the two
/// features are entirely independent passes.
#[test]
fn word_characters_are_never_treated_as_brackets() {
    assert!(!is_bracket('a'));
    assert!(!is_bracket('_'));
    assert!(!is_bracket('9'));
}

/// Symmetric case to `no_highlights_for_an_unmatched_bracket`
/// (which only covers an unmatched *opening* bracket, scanning
/// forward) -- a closing bracket with nothing before it to open it
/// must also come back empty, not panic or match something wrong.
#[test]
fn no_highlights_for_an_unmatched_closing_bracket() {
    let lines = Lines::from("a + b)");
    let highlights = bracket_match_highlights(&lines, Index2::new(0, 5), style());

    assert!(highlights.is_empty());
}

/// A cursor row past the end of the buffer must not panic --
/// `lines.get` returns `None` and `bracket_at` propagates that via
/// `?`, same as any other out-of-range lookup elsewhere in this
/// codebase (`word_highlight::word_at`'s own `does_not_panic_when_
/// the_cursor_column_is_stale_on_an_empty_row` is the sibling
/// regression test this mirrors).
#[test]
fn does_not_panic_when_the_cursor_row_is_past_the_end_of_the_buffer() {
    let lines = Lines::from("(a)");
    let highlights = bracket_match_highlights(&lines, Index2::new(5, 0), style());

    assert!(highlights.is_empty());
}

/// Same class of guard, for a cursor column far past the end of a
/// real row (can happen after `MoveUp`/`MoveDown` from a longer
/// line, per the same `word_at` history referenced above) -- must
/// not panic on `row[cursor.col]`/`row[cursor.col - 1]`.
#[test]
fn does_not_panic_when_the_cursor_column_is_far_past_the_end_of_the_row() {
    let lines = Lines::from("(a)");
    let highlights = bracket_match_highlights(&lines, Index2::new(0, 99), style());

    assert!(highlights.is_empty());
}

#[test]
fn no_highlights_on_a_completely_empty_line() {
    let lines = Lines::from("");
    let highlights = bracket_match_highlights(&lines, Index2::new(0, 0), style());

    assert!(highlights.is_empty());
}

/// Two side-by-side pairs of the same kind -- the cursor on the
/// *second* opening bracket must match the second closing bracket,
/// not accidentally pair across the two independent groups (e.g.
/// matching the first pair's own close, or the first pair's open
/// matching the second pair's close).
#[test]
fn matches_the_correct_pair_among_two_adjacent_same_kind_pairs() {
    let lines = Lines::from("()()"); // columns: ( ) ( )
    let highlights = bracket_match_highlights(&lines, Index2::new(0, 2), style());

    assert_eq!(
        highlights,
        vec![Highlight::new(Index2::new(0, 2), Index2::new(0, 2), style()), Highlight::new(Index2::new(0, 3), Index2::new(0, 3), style())]
    );
}

/// Cursor directly on a bracket character takes priority over the
/// "touching from the left" fallback -- with two adjacent brackets,
/// standing on the second one (itself a bracket) must never be
/// reinterpreted as touching the first one from the right.
#[test]
fn cursor_on_a_bracket_is_never_reinterpreted_as_touching_the_previous_one() {
    let lines = Lines::from("()");
    let highlights = bracket_match_highlights(&lines, Index2::new(0, 1), style());

    // Column 1 is ')' itself -- its match is the '(' at column 0,
    // not itself (which "touching from the left" would wrongly
    // resolve to, if the cursor's own cell weren't checked first).
    assert_eq!(
        highlights,
        vec![Highlight::new(Index2::new(0, 1), Index2::new(0, 1), style()), Highlight::new(Index2::new(0, 0), Index2::new(0, 0), style())]
    );
}

#[test]
fn matches_deeply_nested_same_kind_brackets() {
    let lines = Lines::from("(((())))"); // 4 levels deep
    // The outermost '(' (column 0) must match the outermost ')'
    // (column 7), not any of the inner ones.
    let highlights = bracket_match_highlights(&lines, Index2::new(0, 0), style());
    assert_eq!(
        highlights,
        vec![Highlight::new(Index2::new(0, 0), Index2::new(0, 0), style()), Highlight::new(Index2::new(0, 7), Index2::new(0, 7), style())]
    );

    // The innermost '(' (column 3) must match the innermost ')'
    // (column 4).
    let highlights = bracket_match_highlights(&lines, Index2::new(0, 3), style());
    assert_eq!(
        highlights,
        vec![Highlight::new(Index2::new(0, 3), Index2::new(0, 3), style()), Highlight::new(Index2::new(0, 4), Index2::new(0, 4), style())]
    );
}

/// Symmetric case to `ignores_a_different_bracket_kind_while_nested`
/// (which only scans forward, from the opening side) -- scanning
/// backward from a closing bracket must also skip over an unrelated
/// bracket kind sitting between it and its real match, rather than
/// stopping depth-tracking on it.
#[test]
fn ignores_a_different_bracket_kind_while_nested_scanning_backward() {
    let lines = Lines::from("([)]");
    // The ']' at column 3 must match the '[' at column 1, not the
    // ')'/'(' pair around it.
    let highlights = bracket_match_highlights(&lines, Index2::new(0, 3), style());

    assert_eq!(
        highlights,
        vec![Highlight::new(Index2::new(0, 3), Index2::new(0, 3), style()), Highlight::new(Index2::new(0, 1), Index2::new(0, 1), style())]
    );
}

/// A same-kind scan must also skip over an *unmatched, unrelated*
/// bracket character sitting in between -- not just a properly
/// paired one (`ignores_a_different_bracket_kind_while_nested`
/// above). Confirms a stray foreign bracket never terminates or
/// otherwise disrupts the depth count for the kind actually being
/// searched.
#[test]
fn a_stray_unrelated_bracket_kind_does_not_disrupt_the_scan() {
    let lines = Lines::from("(a]b)"); // a lone, unmatched ']' inside
    let highlights = bracket_match_highlights(&lines, Index2::new(0, 0), style());

    assert_eq!(
        highlights,
        vec![Highlight::new(Index2::new(0, 0), Index2::new(0, 0), style()), Highlight::new(Index2::new(0, 4), Index2::new(0, 4), style())]
    );
}

/// Symmetric case to `matches_across_multiple_lines` (which only
/// covers the forward/opening-side scan) -- backward search must
/// also correctly cross row boundaries.
#[test]
fn matches_across_multiple_lines_scanning_backward() {
    let lines = Lines::from("fn main() {\n    let x = 1;\n}");
    // Cursor on the '}' at row 2 -- must match the '{' at column 10
    // of row 0 ("fn main() {" -- f-n-space-m-a-i-n-(-)-space-{).
    let highlights = bracket_match_highlights(&lines, Index2::new(2, 0), style());

    assert_eq!(
        highlights,
        vec![Highlight::new(Index2::new(2, 0), Index2::new(2, 0), style()), Highlight::new(Index2::new(0, 10), Index2::new(0, 10), style())]
    );
}

/// Blank lines sitting between a pair (nothing on them at all, not
/// even whitespace) must not break the row-to-row scan -- both
/// `find_forward`/`find_backward`'s per-row inner loop needs to
/// degrade gracefully to "nothing to look at" on a zero-length row
/// rather than mishandling it.
#[test]
fn matches_across_blank_lines_in_between() {
    let lines = Lines::from("(\n\n\n)");
    let highlights = bracket_match_highlights(&lines, Index2::new(0, 0), style());

    assert_eq!(
        highlights,
        vec![Highlight::new(Index2::new(0, 0), Index2::new(0, 0), style()), Highlight::new(Index2::new(3, 0), Index2::new(3, 0), style())]
    );
}

/// Multi-byte characters before a bracket must not throw off
/// column-based matching -- `Lines` is char-indexed already (this
/// module never touches raw bytes), but this pins that down
/// directly rather than assuming it, the same way `edtui`'s own
/// `split_spans`/selection tests specifically probe emoji content
/// rather than only ASCII.
#[test]
fn matches_correctly_around_multi_byte_characters() {
    let lines = Lines::from("😀café(x)");
    // '(' is the 7th char (0-indexed: 😀,c,a,f,é -- 5 chars -- then
    // '(' at index 5, 'x' at 6, ')' at 7).
    let highlights = bracket_match_highlights(&lines, Index2::new(0, 5), style());

    assert_eq!(
        highlights,
        vec![Highlight::new(Index2::new(0, 5), Index2::new(0, 5), style()), Highlight::new(Index2::new(0, 7), Index2::new(0, 7), style())]
    );
}

/// All four bracket kinds nested inside one another, each cursor
/// position finding exactly its own level's real match -- a single,
/// comprehensive check that same-kind depth tracking for one kind
/// genuinely never leaks into another, across every kind at once
/// rather than pairwise.
#[test]
fn all_four_bracket_kinds_nest_correctly_together() {
    let lines = Lines::from("{[<(a)>]}");
    // Indices: { 0, [ 1, < 2, ( 3, a 4, ) 5, > 6, ] 7, } 8
    assert_eq!(
        bracket_match_highlights(&lines, Index2::new(0, 0), style()),
        vec![Highlight::new(Index2::new(0, 0), Index2::new(0, 0), style()), Highlight::new(Index2::new(0, 8), Index2::new(0, 8), style())],
        "the outermost '{{' should match the outermost '}}'"
    );
    assert_eq!(
        bracket_match_highlights(&lines, Index2::new(0, 1), style()),
        vec![Highlight::new(Index2::new(0, 1), Index2::new(0, 1), style()), Highlight::new(Index2::new(0, 7), Index2::new(0, 7), style())],
        "'[' should match ']'"
    );
    assert_eq!(
        bracket_match_highlights(&lines, Index2::new(0, 2), style()),
        vec![Highlight::new(Index2::new(0, 2), Index2::new(0, 2), style()), Highlight::new(Index2::new(0, 6), Index2::new(0, 6), style())],
        "'<' should match '>'"
    );
    assert_eq!(
        bracket_match_highlights(&lines, Index2::new(0, 3), style()),
        vec![Highlight::new(Index2::new(0, 3), Index2::new(0, 3), style()), Highlight::new(Index2::new(0, 5), Index2::new(0, 5), style())],
        "the innermost '(' should match the innermost ')'"
    );
}

#[test]
fn cursor_is_on_a_matched_bracket_true_when_sitting_directly_on_a_matched_open_or_close() {
    let lines = Lines::from("(a)");
    assert!(cursor_is_on_a_matched_bracket(&lines, Index2::new(0, 0)));
    assert!(cursor_is_on_a_matched_bracket(&lines, Index2::new(0, 2)));
}

#[test]
fn cursor_is_on_a_matched_bracket_false_for_an_unmatched_bracket() {
    let lines = Lines::from("(a");
    assert!(!cursor_is_on_a_matched_bracket(&lines, Index2::new(0, 0)));
}

#[test]
fn cursor_is_on_a_matched_bracket_false_when_not_touching_a_bracket_at_all() {
    let lines = Lines::from("hello");
    assert!(!cursor_is_on_a_matched_bracket(&lines, Index2::new(0, 2)));
}

/// The one case that must come back `false` even though `bracket_at`
/// itself would resolve something: the cursor sitting one column
/// *past* a bracket (the "touching from the left" append position)
/// isn't genuinely *on* a bracket character -- painting that cell
/// with the highlight color would color an empty/unrelated cell,
/// not the bracket itself. `bracket_match_highlights` is fine
/// returning a highlight anchored at the real bracket position
/// either way; it's specifically the terminal *cursor cell*
/// (`Editor::view`'s `cursor_style` decision) that must stay plain
/// here.
#[test]
fn cursor_is_on_a_matched_bracket_false_when_only_touching_from_the_append_position() {
    let lines = Lines::from("(a)");
    assert!(!cursor_is_on_a_matched_bracket(&lines, Index2::new(0, 3)));
}
