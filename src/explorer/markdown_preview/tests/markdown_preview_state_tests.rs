use super::*;

#[test]
fn open_fails_for_a_non_markdown_path() {
    let dir = unique_scratch_dir("markdown-preview");
    let path = dir.join("notes.txt");
    fs::write(&path, "hi").unwrap();

    assert!(MarkdownPreviewState::open(&path).is_none());
}

#[test]
fn open_reads_and_renders_a_real_file() {
    let dir = unique_scratch_dir("markdown-preview");
    let path = dir.join("readme.md");
    fs::write(&path, "# Title\n\nSome text.\n").unwrap();

    let state = MarkdownPreviewState::open(&path).unwrap();

    assert_eq!(state.path(), path);
    assert!(!state.lines().is_empty());
}

#[test]
fn scroll_down_stops_at_the_last_line() {
    let dir = unique_scratch_dir("markdown-preview");
    let path = dir.join("readme.md");
    fs::write(&path, "a\n\nb\n").unwrap();
    let mut state = MarkdownPreviewState::open(&path).unwrap();
    let last = state.lines().len() - 1;

    for _ in 0..last + 5 {
        state.scroll_down();
    }

    assert_eq!(state.scroll(), last);
}

#[test]
fn scroll_up_stops_at_zero() {
    let dir = unique_scratch_dir("markdown-preview");
    let path = dir.join("readme.md");
    fs::write(&path, "a\n").unwrap();
    let mut state = MarkdownPreviewState::open(&path).unwrap();

    state.scroll_up();

    assert_eq!(state.scroll(), 0);
}

#[test]
fn page_down_advances_by_a_page_and_clamps_at_the_last_line() {
    let dir = unique_scratch_dir("markdown-preview");
    let path = dir.join("readme.md");
    // One rendered line per paragraph -- enough blank-line-separated
    // paragraphs that a single PAGE_SIZE (15) page_down doesn't
    // already reach the end, so this actually exercises the "advance
    // by a page" arithmetic and not just the clamp.
    let content = (0..30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n\n");
    fs::write(&path, content).unwrap();
    let mut state = MarkdownPreviewState::open(&path).unwrap();
    let last = state.lines().len() - 1;
    assert!(last > 15, "fixture should be taller than one page");

    state.page_down();
    assert_eq!(state.scroll(), 15, "should advance by exactly one page");

    for _ in 0..10 {
        state.page_down();
    }
    assert_eq!(state.scroll(), last, "repeated page_down should clamp at the last line, not overshoot");
}

#[test]
fn page_up_retreats_by_a_page_and_clamps_at_zero() {
    let dir = unique_scratch_dir("markdown-preview");
    let path = dir.join("readme.md");
    let content = (0..30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n\n");
    fs::write(&path, content).unwrap();
    let mut state = MarkdownPreviewState::open(&path).unwrap();
    state.page_down();
    state.page_down(); // scroll = 30

    state.page_up();
    assert_eq!(state.scroll(), 15, "should retreat by exactly one page");

    state.page_up();
    state.page_up();
    assert_eq!(state.scroll(), 0, "repeated page_up should clamp at zero, not underflow");
}

/// The actual point of `reload`: re-reading the file after a save
/// picks up the new content instead of showing stale rendered
/// lines.
#[test]
fn reload_picks_up_the_files_new_content() {
    let dir = unique_scratch_dir("markdown-preview");
    let path = dir.join("readme.md");
    fs::write(&path, "old\n").unwrap();
    let mut state = MarkdownPreviewState::open(&path).unwrap();

    fs::write(&path, "brand new content\n").unwrap();
    state.reload();

    let text: String = state.lines().iter().flatten().map(|span| span.text.as_str()).collect();
    assert!(text.contains("brand new content"), "should reflect the file's new content: {text:?}");
}

/// Regression coverage for the actual point of clamping rather than
/// resetting to `0` on reload: editing near the end of a long
/// document and saving shouldn't jump the preview back to the top,
/// but a scroll position past a now-*shorter* document has to be
/// pulled back in bounds rather than left pointing past the end.
#[test]
fn reload_clamps_the_scroll_position_to_the_new_shorter_content() {
    let dir = unique_scratch_dir("markdown-preview");
    let path = dir.join("readme.md");
    let long_content = (0..30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n\n");
    fs::write(&path, &long_content).unwrap();
    let mut state = MarkdownPreviewState::open(&path).unwrap();
    for _ in 0..40 {
        state.scroll_down();
    }
    let scroll_before = state.scroll();
    assert!(scroll_before > 2, "sanity: scrolled well past a 3-line document");

    fs::write(&path, "a\n\nb\n\nc\n").unwrap(); // much shorter now
    state.reload();

    assert_eq!(state.scroll(), state.lines().len() - 1, "scroll should be pulled back in bounds, not left pointing past the new, shorter content");
}

/// `reload` on a file that's vanished (or otherwise fails to read)
/// must leave the previously-good preview untouched rather than
/// blanking it -- same "never blocks on this" convention the rest
/// of this module follows.
#[test]
fn reload_leaves_the_preview_untouched_on_a_read_failure() {
    let dir = unique_scratch_dir("markdown-preview");
    let path = dir.join("readme.md");
    fs::write(&path, "still here\n").unwrap();
    let mut state = MarkdownPreviewState::open(&path).unwrap();
    let lines_before = state.lines().len();

    fs::remove_file(&path).unwrap();
    state.reload();

    assert_eq!(state.lines().len(), lines_before, "should keep showing the last-good content, not blank out");
}

/// The actual point of the whole click-a-link feature: a click
/// landing on the rendered row holding the link finds its URL.
/// `visible_row_links` is populated by hand here, matching what
/// `ui::markdown_preview::draw_markdown_preview` would actually
/// build for this content -- `link_at` itself only ever reads that
/// table, it doesn't re-derive it from `state.lines()`.
#[test]
fn link_at_finds_the_url_on_the_clicked_line() {
    let dir = unique_scratch_dir("markdown-preview");
    let path = dir.join("readme.md");
    fs::write(&path, "[Anthropic](https://anthropic.com)\n").unwrap();
    let mut state = MarkdownPreviewState::open(&path).unwrap();
    state.set_content_area(0, 0, 80, 24);
    state.set_visible_row_links(vec![vec![(0, "Anthropic".chars().count() as u16, "https://anthropic.com".to_string())]]);

    assert_eq!(state.link_at(0, 0), Some("https://anthropic.com"));
}

#[test]
fn link_at_is_none_before_the_first_draw_has_recorded_a_content_area() {
    let dir = unique_scratch_dir("markdown-preview");
    let path = dir.join("readme.md");
    fs::write(&path, "[Anthropic](https://anthropic.com)\n").unwrap();
    let state = MarkdownPreviewState::open(&path).unwrap();

    assert_eq!(state.link_at(0, 0), None);
}

#[test]
fn link_at_is_none_outside_the_content_area() {
    let dir = unique_scratch_dir("markdown-preview");
    let path = dir.join("readme.md");
    fs::write(&path, "[Anthropic](https://anthropic.com)\n").unwrap();
    let mut state = MarkdownPreviewState::open(&path).unwrap();
    state.set_content_area(10, 10, 20, 5);

    assert_eq!(state.link_at(0, 0), None, "click landed to the left of/above the content area");
}

#[test]
fn link_at_is_none_on_a_line_with_no_link() {
    let dir = unique_scratch_dir("markdown-preview");
    let path = dir.join("readme.md");
    fs::write(&path, "just plain text\n").unwrap();
    let mut state = MarkdownPreviewState::open(&path).unwrap();
    state.set_content_area(0, 0, 80, 24);

    assert_eq!(state.link_at(0, 0), None);
}

/// Scrolling changes which logical line lands on visual row 0 --
/// `draw_markdown_preview` re-wraps and rebuilds `visible_row_links`
/// from `state.scroll()` onward every frame, so once scrolled past
/// "line one" and the blank line, row 0's own hitbox table is the
/// link line's, exactly as set here.
#[test]
fn link_at_accounts_for_scroll_offset() {
    let dir = unique_scratch_dir("markdown-preview");
    let path = dir.join("readme.md");
    fs::write(&path, "line one\n\n[link](https://example.com)\n").unwrap();
    let mut state = MarkdownPreviewState::open(&path).unwrap();
    state.set_content_area(0, 0, 80, 24);
    state.scroll_down();
    state.scroll_down();
    state.set_visible_row_links(vec![vec![(0, "link".chars().count() as u16, "https://example.com".to_string())]]);

    assert_eq!(state.link_at(0, 0), Some("https://example.com"), "row 0 on screen should now be the scrolled-to line holding the link");
}

fn line_text_at(state: &MarkdownPreviewState, index: usize) -> String {
    state.lines()[index].iter().map(|span| span.text.as_str()).collect()
}

/// The actual point of the whole sync feature: moving the editor's
/// cursor onto a source line highlights the rendered line it became
/// and scrolls it into view.
#[test]
fn sync_to_editor_cursor_highlights_and_scrolls_to_the_matching_line() {
    let dir = unique_scratch_dir("markdown-preview");
    let path = dir.join("readme.md");
    fs::write(&path, "first\n\nsecond\n\nthird\n").unwrap();
    let mut state = MarkdownPreviewState::open(&path).unwrap();

    state.sync_to_editor_cursor(2, 0.0, 24); // "second" starts at source line 2

    let index = state.highlighted_line().expect("should have matched a line");
    assert_eq!(line_text_at(&state, index), "second");
    assert_eq!(state.scroll(), index);
}

/// The actual point of the follow-up request: a cursor positioned
/// halfway down the *editor's* own visible page should scroll the
/// preview so the matched line lands halfway down *its* own page
/// too, not always snapped to the top.
#[test]
fn sync_to_editor_cursor_offsets_the_scroll_by_the_relative_position() {
    let dir = unique_scratch_dir("markdown-preview");
    let path = dir.join("readme.md");
    let content: String = (0..20).map(|i| format!("line {i}\n\n")).collect();
    fs::write(&path, &content).unwrap();
    let mut state = MarkdownPreviewState::open(&path).unwrap();

    // "line 10" starts at source row 20 (each paragraph plus its
    // own trailing blank separator takes 2 source lines).
    state.sync_to_editor_cursor(20, 0.5, 10);

    let index = state.highlighted_line().expect("should have matched a line");
    assert_eq!(line_text_at(&state, index), "line 10");
    assert_eq!(state.scroll(), index - 5, "should scroll back by half the given visible height");
}

/// A cursor row with no rendered line of its own (a blank separator
/// line between two paragraphs) should still highlight *something*
/// sensible -- the closest preceding real content, not nothing and
/// not the next paragraph down.
#[test]
fn sync_to_editor_cursor_falls_back_to_the_closest_preceding_line() {
    let dir = unique_scratch_dir("markdown-preview");
    let path = dir.join("readme.md");
    fs::write(&path, "first\n\nsecond\n").unwrap();
    let mut state = MarkdownPreviewState::open(&path).unwrap();

    state.sync_to_editor_cursor(1, 0.0, 24); // the blank line between "first" and "second"

    let index = state.highlighted_line().unwrap();
    assert_eq!(line_text_at(&state, index), "first");
}

#[test]
fn sync_to_editor_cursor_is_a_noop_on_an_empty_document() {
    let dir = unique_scratch_dir("markdown-preview");
    let path = dir.join("readme.md");
    fs::write(&path, "").unwrap();
    let mut state = MarkdownPreviewState::open(&path).unwrap();

    state.sync_to_editor_cursor(0, 0.0, 24);

    assert_eq!(state.highlighted_line(), None);
}
