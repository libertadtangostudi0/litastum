use super::*;
use crate::editor::syntax::resolve_syntax;

fn cache_for(file_name: &str, extension: &str) -> SyntaxCache {
    let (syntax_set, syntax, theme) = resolve_syntax(&[file_name, extension], "", &None).expect("a grammar for the file");
    SyntaxCache::new(syntax_set, syntax, theme)
}

/// The styles over each character of `row`, `None` where plain.
fn styles_on(highlights: &[Highlight], lines: &Lines, row: usize) -> Vec<Option<Style>> {
    (0..row_of(lines, row).len()).map(|col| highlights.iter().find(|highlight| highlight.contains(&Index2::new(row, col))).map(|highlight| highlight.style)).collect()
}

/// Not a color any theme uses: every piece of text gets a highlight.
const BASE: Color = Color::Indexed(250);

const C_WITH_A_BLOCK_COMMENT: &str = "int x;\n/*\n  register for while\n*/\nint y;";

/// Reported: inside a block comment, `register` and `for` were colored as
/// keywords -- `edtui` started every line afresh.
#[test]
fn a_block_comment_stays_a_comment_on_its_later_lines() {
    let lines = Lines::from(C_WITH_A_BLOCK_COMMENT);
    let mut cache = cache_for("x.c", "c");
    let highlights = cache.highlights(&lines, 0..5, BASE);

    let opening = styles_on(&highlights, &lines, 1)[0].expect("`/*` is colored");
    let inside = styles_on(&highlights, &lines, 2);
    assert!(inside.iter().skip(2).all(|style| *style == Some(opening)), "every word in the comment as a comment: {inside:?}");
    let after = styles_on(&highlights, &lines, 4);
    assert_ne!(after[0], Some(opening), "`int` after the comment is code again");
}

#[test]
fn rows_far_below_still_know_they_are_in_a_comment() {
    let mut text = String::from("/*\n");
    for _ in 0..300 {
        text.push_str("for int\n");
    }
    let lines = Lines::from(text.as_str());
    let mut cache = cache_for("x.c", "c");
    let comment = styles_on(&cache.highlights(&lines, 0..1, BASE), &lines, 0)[0];

    let far = cache.highlights(&lines, 290..292, BASE);
    assert!(styles_on(&far, &lines, 290).iter().all(|style| *style == comment), "from the kept states, past several checkpoints");
}

/// A change above the rows on screen recolors them: the states under it
/// are dropped, by its row or, unknown, by the rows' hashes.
#[test]
fn opening_a_comment_above_recolors_what_follows() {
    let code = "int a;\n".repeat(200);
    let mut cache = cache_for("x.c", "c");
    let before = Lines::from(code.as_str());
    let plain = styles_on(&cache.highlights(&before, 150..151, BASE), &before, 150);

    let commented = Lines::from(format!("/*{code}").as_str());
    cache.row_changed(0);
    let after = styles_on(&cache.highlights(&commented, 150..151, BASE), &commented, 150);
    assert_ne!(after, plain, "row 150 is in the comment now");

    let mut cache = cache_for("x.c", "c");
    cache.highlights(&before, 150..151, BASE);
    cache.buffer_changed(&commented);
    assert_eq!(styles_on(&cache.highlights(&commented, 150..151, BASE), &commented, 150), after, "found by the hashes");
}

/// A view far down a large file doesn't parse all of it in one frame
/// (a jump to the end of 100k lines took 2 s): the frames after it catch
/// up, the event loop drawing them (`catching_up`).
#[test]
fn a_view_far_down_catches_up_over_frames() {
    let lines = Lines::from(format!("/*\n{}", "for\n".repeat(1000)).as_str());
    let mut cache = cache_for("x.c", "c");
    let comment = styles_on(&cache.highlights(&lines, 0..1, BASE), &lines, 0)[0];
    let last = 999;

    cache.budget = Duration::ZERO;
    let first_frame = styles_on(&cache.highlights(&lines, last..last + 1, BASE), &lines, last);
    assert!(cache.catching_up());
    assert_ne!(first_frame[0], comment, "not parsed that far yet: a clean state");

    cache.budget = Duration::from_secs(60);
    assert_eq!(styles_on(&cache.highlights(&lines, last..last + 1, BASE), &lines, last)[0], comment);
    assert!(!cache.catching_up());
}


/// Colored rows are kept: an edit above them recolors them, one below
/// leaves them be, and an edit whose row isn't known drops what can't be
/// checked.
#[test]
fn kept_rows_go_with_an_edit_above_them() {
    let code = "int a;\n".repeat(100);
    let lines = Lines::from(code.as_str());
    let mut cache = cache_for("x.c", "c");
    cache.highlights(&lines, 40..60, BASE);
    assert!(cache.rows.contains_key(&50));

    cache.row_changed(70);
    assert!(cache.rows.contains_key(&50), "below it: kept");
    cache.row_changed(45);
    assert!(!cache.rows.contains_key(&50), "above it: recolored");
    assert!(cache.rows.contains_key(&44));

    cache.buffer_changed(&lines);
    assert!(!cache.rows.contains_key(&44), "nothing hashed that far: dropped");
}


#[test]
fn neighbouring_pieces_of_one_style_are_one_highlight() {
    let lines = Lines::from("// one two three");
    let mut cache = cache_for("x.c", "c");
    let highlights = cache.highlights(&lines, 0..1, BASE);
    assert_eq!(highlights.len(), 1, "{highlights:?}");
    assert_eq!((highlights[0].start.col, highlights[0].end.col), (0, 15));
}


/// Reported by a probe: typing past the last checkpoint, with the rows
/// above it on screen still kept, looked up a checkpoint that wasn't
/// there and panicked.
#[test]
fn an_edit_low_in_the_view_with_kept_rows_above_it() {
    let lines = Lines::from("int a;\n".repeat(300).as_str());
    let mut cache = cache_for("x.c", "c");
    cache.highlights(&lines, 100..200, BASE);

    cache.row_changed(190);
    let highlights = cache.highlights(&lines, 100..200, BASE);
    assert!(highlights.iter().any(|highlight| highlight.start.row == 195), "the rows under the edit are colored again");
}


/// An edit on a colored row starts parsing right at it, from the state
/// kept with it: typing cost a reparse from the checkpoint above.
#[test]
fn an_edit_on_a_colored_row_resumes_from_its_own_state() {
    let lines = Lines::from("int a;\n".repeat(300).as_str());
    let mut cache = cache_for("x.c", "c");
    cache.highlights(&lines, 100..160, BASE);

    cache.row_changed(130);
    assert_eq!(cache.resume.as_ref().map(|(at, _)| *at), Some(130));
}
